//! A card's PR (4.11.1, 4.11.6): its candidate pushed, the PR opened in the operator's name, held while its wording is
//! incomplete (fac-296), and read. Every PR merges on his approval alone (fac-98, fac-111): published, it waits for his.

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::AskKind;
use mork_types::card::{Card, Landing, Mode, Stage};
use mork_types::config::Config;
use mork_types::work::{PrSeen, PrState};
use mork_types::{CardId, Sha};
use serde_json::Value;

use super::landing::alarm;
use crate::decide::{Ctx, Decision};
use crate::messages::asks;
use crate::post::Poster;
use crate::state::State;

/// A card's repository on Bitbucket, `workspace/slug`, or its project's name.
pub fn repo_of(c: &Card, config: &Config) -> String {
    config.projects.get(&c.project).and_then(|p| p.forge.repo_key()).unwrap_or_else(|| c.project.to_string())
}

// The most words a PR's title keeps of the card's.
const TITLE_WORDS: usize = 6;

pub(crate) fn title_words(state: &State, c: &Card, config: &Config) -> String {
    let title = state.board.get(&c.id).map_or(c.title.clone(), |b| b.title.clone());
    let lead = title.split(':').next().filter(|l| !l.trim().is_empty()).unwrap_or(&title);
    let is_ticket =
        |w: &&str| w.strip_prefix('#').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let own = !crate::board::tickets::own(state, c, config).is_empty();
    let words: Vec<&str> = lead.split_whitespace().skip_while(|w| own && is_ticket(w)).take(TITLE_WORDS).collect();
    words.join(" ").trim_end_matches(|ch: char| !ch.is_alphanumeric()).to_owned()
}

/// A card's PR title: its board title cut at its first colon, a few words, no card id (fac-111); a ticketed project's
/// starts with its tickets, then its foreman's uncropped `PR wording` summary when it has one (fac-296).
pub fn pr_title(state: &State, c: &Card, config: &Config) -> String {
    let words = crate::board::tickets::summary(state, c, config).unwrap_or_else(|| title_words(state, c, config));
    let tickets: Vec<String> = crate::board::tickets::of(state, c, config).iter().map(|n| format!("#{n}")).collect();
    if tickets.is_empty() { words } else { format!("{} {words}", tickets.join(" ")).trim_end().to_owned() }
}

// A card that left the queue meanwhile records its push and PR opening and goes no further.
pub(super) fn published(c: &Card, commit: &Sha) -> bool {
    c.stage == Stage::Merging
        && matches!(c.landing, Some(Landing::Published { .. }))
        && super::order::commit_of(c).as_ref() == Some(commit)
}

/// The card's head pushed: its PR opens, or the merge goes on with the PR it has.
pub fn pushed(p: &mut Poster<'_>, mut c: Card, commit: &Sha, config: &Config) {
    if c.pushed.as_ref().is_some_and(|old| old != commit) {
        c.previous_push = c.pushed.clone();
        c.gates.approved = None;
        asks::close(p, c.id.as_str(), &[AskKind::ApprovePr]);
    }
    c.pushed = Some(commit.clone());
    // The card's head it was made of: a later one descends from it, never from the candidate.
    c.pushed_head = c.head.clone();
    let id = c.id.clone();
    p.put_card(c.clone());
    if !published(&c, commit) {
        return;
    }
    let branch = c.branch.clone().unwrap_or_else(|| id.as_str().to_owned());
    let (title, description) = (pr_title(p.state, &c, config), crate::board::tickets::description(p.state, &c, config));
    let what = ActionWhat::OpenPr { card: id, project: c.project.clone(), branch, title, description, pr: c.pr };
    // Published again: the card's own PR takes the title as it is now, as a new opening naming it,
    // then waits again.
    let opening = p.act_as(what, c.pr.is_some());
    hold(p, &opening, config);
}

/// The PR opened, in the operator's name: it waits for his approval.
pub fn pr_opened(p: &mut Poster<'_>, mut c: Card, v: &Value, ctx: Ctx<'_>) {
    let Some(pr) = v.get("pr").and_then(Value::as_u64) else { return };
    c.pr = Some(pr);
    p.put_card(c.clone());
    if c.pushed.as_ref().is_some_and(|x| published(&c, x)) {
        super::approval::wait(p, c, ctx);
    }
}

/// A PR read, by the poller or right before the merge call (4.11.1, 4.11.6).
pub fn pr_read(state: &State, card: &CardId, seen: &PrSeen, before_merge: bool, ctx: Ctx<'_>) -> Decision {
    let Some(c) = state.cards.get(card).cloned() else { return Decision::default() };
    let mut p = Poster::for_ctx(state, ctx);
    if c.mode == Mode::Review && seen.state != PrState::Open {
        crate::reviews::end::closed(&mut p, c, seen, ctx);
    } else if c.mode == Mode::Review {
        super::approval::colleague(&mut p, &c, seen, ctx);
    } else {
        read(&mut p, c, seen, before_merge, ctx);
    }
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// A PR read after a merge's guard dropped the merge (2.5.8), its slot held (fac-10): what moved outside is alarmed;
/// else the base is fetched again and the merge goes on, a moved base validated again first.
pub fn reread(p: &mut Poster<'_>, mut c: Card, seen: &PrSeen, ctx: Ctx<'_>) {
    if let Some(Landing::Merge { tree, base, commit }) = c.landing.clone() {
        c.landing = Some(Landing::Published { tree, base, commit, verified: false });
        c.gates.approved = None;
        p.put_card(c.clone());
    }
    let id = c.id.clone();
    read(p, c, seen, false, ctx);
    if let Some(c) = p.card(&id).filter(|c| !c.alarm && matches!(c.landing, Some(Landing::Published { .. }))) {
        super::approval::wait(p, c, ctx);
    }
}

fn read(p: &mut Poster<'_>, mut c: Card, seen: &PrSeen, before_merge: bool, ctx: Ctx<'_>) {
    // A read of a PR the card no longer holds, an earlier run's, is nobody's (fac-171).
    if c.pr != Some(seen.pr) {
        return;
    }
    if seen.state == PrState::Open && c.previous_push.as_ref() == Some(&seen.head) {
        return;
    }
    super::approval::approval_read(p, &mut c, seen, ctx);
    // An earlier run's PR is this run's once read open, or merged on this run's push, a merged
    // head being frozen; else the run lets it go, keeping what its branch may still hold (fac-171).
    if c.pr_unconfirmed {
        let ours = c.pushed.as_ref().filter(|x| c.carried_push.as_ref() != Some(*x));
        let merged_ours = seen.state == PrState::Merged && ours == Some(&seen.head);
        (c.pr_unconfirmed, c.carried_push) = (false, None);
        if seen.state != PrState::Open && !merged_ours {
            c.pr = None;
        }
        p.put_card(c.clone());
        if c.pr.is_none() {
            return;
        }
    }
    // Merged, any live card lands, alarmed or back for a fix (fac-171); else only a landing reads it.
    let merged = seen.state == PrState::Merged && c.stage.is_live();
    if !merged && (c.alarm || !matches!(c.stage, Stage::Ready | Stage::Merging)) {
        return;
    }
    let forge = ctx.config.forge_label(&c.project);
    let pr = seen.pr;
    // What lands is the train's candidate, which is what was pushed (fac-68); merged by hand, the
    // card holds none any more, and what it pushed is what landed (fac-98).
    let hand = c.pushed.clone().filter(|_| c.landing == Some(Landing::HandMerged));
    let head = hand.or_else(|| super::order::commit_of(&c)).map(|h| h.0).unwrap_or_default();
    // The PR's head is what Mork pushed last: a card back from the queue may have a newer
    // head it hasn't pushed yet (fac-10).
    let ours = c.pushed.clone().map_or_else(|| head.clone(), |x| x.0);
    let same = seen.head.0 == ours;
    match seen.state {
        PrState::Open if !same => alarm(p, c, &format!("PR {pr}'s head moved outside Mork")),
        // A PR waiting out of the train merges on the operator's approval alone (fac-98).
        PrState::Open if super::approval::waiting(&c) => super::approval::own(p, &c, seen, ctx),
        PrState::Open => {}
        // The merge call is under way, an approved one's too: its own result decides (4.11.2.5, R7).
        PrState::Merged if !before_merge && (matches!(c.landing, Some(Landing::Merge { .. })) || calling(p, &c.id)) => {
        }
        PrState::Merged => super::pr_merged::read(p, c, seen, &head, ctx),
        PrState::Declined | PrState::Superseded => alarm(p, c, &format!("PR {pr} was declined on {forge}")),
    }
}

// Whether an approved merge of the card is being called; one still waiting gives way to a hand merge (fac-98).
fn calling(p: &Poster<'_>, id: &CardId) -> bool {
    let merge = |a: &Action| matches!(a.what, ActionWhat::Merge { ref card, .. } if card == id);
    p.all_actions().any(|a| merge(a) && a.status == ActionStatus::Started)
}

/// Why a PR the leak or internals check refused is held: its title's words.
pub fn held_words(words: &str) -> String {
    format!(
        "{words}: on a project with tickets configured, change the card's `PR wording` note with card.change --note; otherwise change its board title with card.change --title; then /action.retry"
    )
}

// Why a PR opening can't go yet: its card's `PR wording` note is missing or incomplete (fac-296).
fn unworded(state: &State, a: &Action, config: &Config) -> Option<String> {
    let ActionWhat::OpenPr { card, .. } = &a.what else { return None };
    state.cards.get(card).and_then(|c| crate::board::tickets::unworded(state, c, config))
}

// A PR opening just made is held when its wording is incomplete, with what to write (fac-296);
// nothing runs it before.
fn hold(p: &mut Poster<'_>, id: &mork_types::ActionId, config: &Config) {
    let a = p
        .all_actions()
        .find(|a| &a.id == id && matches!(a.status, ActionStatus::Pending | ActionStatus::Held))
        .cloned();
    if let Some((a, why)) = a.and_then(|a| unworded(p.state, &a, config).map(|why| (a, why))) {
        held(p, a, &why);
    }
}

// The foreman writes the wording, so the hold goes to it, not to the operator: no ask is raised.
fn held(p: &mut Poster<'_>, mut a: Action, why: &str) {
    a.status = ActionStatus::Held;
    a.words = Some(why.to_owned());
    let (id, card) = (a.id.clone(), a.what.card().cloned());
    p.put_action(a);
    if let Some(card) = card {
        let text = format!("Opening the PR is held: {why}. Run it with `mork ctl action.retry {id}`.");
        crate::messages::requests::ask_agent(p, &card, mork_types::reply::Role::Foreman, "Write the PR wording", &text);
    }
}

/// A pending action about to run: a PR opening takes its title and description as they are now
/// (2.5.7, M3.17; fac-98), held again with what to write when its wording is incomplete (fac-296).
pub fn run(p: &mut Poster<'_>, mut a: Action, config: &Config) {
    if let ActionWhat::OpenPr { card, title, description, .. } = &mut a.what
        && let Some(c) = p.state.cards.get(card)
    {
        *title = pr_title(p.state, c, config);
        *description = crate::board::tickets::description(p.state, c, config);
    }
    if let Some(why) = unworded(p.state, &a, config) {
        return held(p, a, &why);
    }
    a.status = ActionStatus::Pending;
    let id = a.id.clone();
    p.put_action(a);
    p.intent(crate::decide::Intent::RunAction { id });
}
