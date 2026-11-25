//! Review cards, of a colleague's PR or branch, or our card's PR reviewed again: made, or reused with their
//! rounds, findings and raisers (fac-98, fac-111), and entered in the train on their fetched head.

use factory_types::ask::{AskKind, Color};
use factory_types::card::{Card, Mode};
use factory_types::review::{Review, Verdict};
use factory_types::state::{MessageKind, Sender};
use factory_types::work::OpenPr;
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

/// A review card's id (4.16.2).
pub fn card_id(project: &str, pr: u64) -> CardId {
    CardId::new(format!("pr-{project}-{pr}"))
}

/// A branch review's card id, its branch's name made an id's.
pub fn branch_id(project: &str, branch: &str) -> CardId {
    let slug: String =
        branch.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    CardId::new(format!("branch-{project}-{slug}"))
}

/// `review <project> <n>`, or Andrei's `review.merge`: a review card of the PR, its diff against
/// the PR's destination, cloned at its source commit.
pub(super) fn review(state: &State, project: &str, n: u64, merge: bool, ctx: Ctx<'_>) -> Decision {
    let Some(cfg) = ctx.config.projects.get(project) else {
        return Decision::refused(format!("no project `{project}`"));
    };
    if cfg.bitbucket.is_none() {
        return Decision::refused(format!("{project} isn't on Bitbucket: it has no PRs to review"));
    }
    let Some(pr) = state.world.open_prs.get(project).and_then(|all| all.iter().find(|x| x.pr == n)).cloned() else {
        return Decision::refused(format!("PR {n} isn't open on {project} as the poller last read it"));
    };
    let id = card_id(project, n);
    if state.cards.get(&id).is_some_and(|c| c.stage.is_live()) {
        return Decision::refused(format!("{id} is live already"));
    }
    let mut p = Poster::for_ctx(state, ctx);
    let dest = Some(pr.dest.clone()).filter(|d| !d.is_empty());
    start(&mut p, project, &pr, Review { merge, dest, ..Review::default() });
    opened(p, &id, project, if merge { "review.merge" } else { "review" })
}

/// A PR's review card made, Andrei's or by itself (fac-98), with its room, its `new-pr` ask closed; an
/// ended one reopens with its rounds, approvals, findings, threads and raisers (fac-111). Its fetch is the caller's.
pub(super) fn start(p: &mut Poster<'_>, project: &str, pr: &OpenPr, review: Review) -> CardId {
    let (n, id) = (pr.pr, card_id(project, pr.pr));
    let prior = p.card(&id);
    let run = prior.as_ref().map_or(1, |c| c.run + 1);
    let delegated = prior.as_ref().is_some_and(|c| c.delegated);
    let self_sustained = prior.as_ref().is_some_and(|c| c.delegated && c.self_sustained);
    let mut review = match prior.and_then(|c| c.review) {
        Some(r) => Review { merge: review.merge || r.merge, auto: review.auto, dest: review.dest, ..carried(*r) },
        None => review,
    };
    copy_context(&mut review, pr);
    let mut c = Card::new(id.clone(), project, &pr.title, Mode::Review, run, p.now);
    let into = review.dest.as_deref().unwrap_or("base");
    c.delegated = delegated || crate::messages::auto::on(p.state, &c.project);
    c.self_sustained = self_sustained;
    c.description = format!("PR {n} from {} ({} → {into})", pr.author, pr.branch);
    c.head = Some(pr.head.clone());
    c.branch = Some(pr.branch.clone());
    c.pr = Some(n);
    c.first_seq = p.state.next_seq;
    let data = json!({"run": run, "mode": "review", "pr": n, "merge": review.merge, "auto": review.auto});
    c.review = Some(Box::new(review));
    p.put_card(c);
    let open: Vec<factory_types::AskId> = p
        .all_asks()
        .filter(|a| a.open && a.kind == AskKind::NewPr && a.text.starts_with(&format!("new PR {n} ")))
        .map(|a| a.id.clone())
        .collect();
    for a in open {
        asks::close_one(p, &a);
    }
    made(p, &id, data);
    id
}

/// Keeps rounds yet to be asked on the latest poll's PR context.
pub(super) fn copy_context(review: &mut Review, pr: &OpenPr) {
    review.description.clone_from(&pr.description);
    review.tickets.clone_from(&pr.tickets);
}

/// Reviews a branch's fetched tip against the project's base, without PR comments. A repeat after
/// a fix continues the same card and review (fac-111).
pub(super) fn branch(state: &State, project: &str, branch: &str, ctx: Ctx<'_>) -> Decision {
    let Some(cfg) = ctx.config.projects.get(project) else {
        return Decision::refused(format!("no project `{project}`"));
    };
    if branch.trim().is_empty() || branch == cfg.base {
        return Decision::refused(format!("`review.branch` reviews a branch other than {project}'s base"));
    }
    let id = branch_id(project, branch);
    if let Some(why) = bound(state, &id) {
        return Decision::refused(why);
    }
    let mut p = Poster::for_ctx(state, ctx);
    let prior = state.cards.get(&id).cloned();
    if let Some(c) = prior.clone().filter(|c| c.stage.is_live()) {
        if c.paused {
            return Decision::refused(format!("{id} is stopped: `resume {id}` first"));
        }
        if crate::queue::order::landing(&c) {
            return Decision::refused(format!("{id} is landing on {}: its review ends once it has", cfg.base));
        }
        crate::queue::entries::repeat(&mut p, c);
        return opened(p, &id, project, "review.branch");
    }
    let run = prior.as_ref().map_or(1, |c| c.run + 1);
    let mut c = Card::new(id.clone(), project, &format!("Branch {branch}"), Mode::Review, run, ctx.now);
    c.delegated = prior.as_ref().is_some_and(|x| x.delegated) || crate::messages::auto::on(p.state, &c.project);
    c.self_sustained = prior.as_ref().is_some_and(|x| x.delegated && x.self_sustained);
    c.description = format!("Branch {branch} (→ {})", cfg.base);
    c.branch = Some(branch.to_owned());
    c.first_seq = p.state.next_seq;
    // An ended review of our card's branch is that card's: none of it is carried into this one.
    let prior = prior.filter(|c| super::gate::subject_of(c).is_none());
    c.review = Some(Box::new(prior.and_then(|c| c.review).map_or_else(Review::default, |r| carried(*r))));
    p.put_card(c);
    made(&mut p, &id, json!({"run": run, "mode": "review", "branch": branch}));
    opened(p, &id, project, "review.branch")
}

// Why a branch card isn't this command's: it reviews our card's branch, or our live card works on
// that branch, whose own review it is (fac-111).
fn bound(state: &State, id: &CardId) -> Option<String> {
    let gate = state.cards.get(id).filter(|c| c.stage.is_live()).and_then(super::gate::subject_of);
    if let Some(of) = gate {
        return Some(format!("{id} reviews {of}'s branch: it ends with {of}"));
    }
    let live = |c: &&Card| c.mode != Mode::Review && c.stage.is_live() && super::gate::id_of(c) == *id;
    let ours = state.cards.values().find(live)?;
    Some(format!("{id} is {}'s branch: its review is {}'s own", ours.id, ours.id))
}

// An ended branch card's review, carried into its next run: its rounds, what they approved and the
// findings not verified fixed; no run finished, and no round under way.
fn carried(mut r: Review) -> Review {
    r.finished = None;
    r.next = None;
    for x in r.rounds.iter_mut().filter(|x| x.verdict.is_none()) {
        x.verdict = Some(factory_types::review::Verdict::Superseded);
    }
    r
}

/// An ended PR card reopened as our card's gate (fac-375), a run after its last, whose clone removal then leaves the new
/// clone; its rounds, findings and raisers carried, its kept seats resumed.
pub(super) fn reopen(p: &mut Poster<'_>, prior: Card, base: &Sha, ctx: Ctx<'_>) -> Card {
    let mut c = Card::new(prior.id.clone(), prior.project.as_str(), &prior.title, Mode::Review, prior.run + 1, ctx.now);
    c.delegated = prior.delegated || crate::messages::auto::on(p.state, &c.project);
    c.self_sustained = prior.delegated && prior.self_sustained;
    c.description = prior.description;
    c.branch = prior.branch;
    c.pr = prior.pr;
    c.base_tip = Some(base.clone());
    c.first_seq = p.state.next_seq;
    c.review = prior.review.map(|r| Box::new(carried(*r)));
    p.put_card(c.clone());
    made(p, &c.id, json!({"run": c.run, "mode": "review", "pr": c.pr}));
    c
}

// A colleague's review card is made: its room, and its start journaled.
fn made(p: &mut Poster<'_>, id: &CardId, data: Value) {
    p.touch_place(id.as_str());
    crate::lifecycle::stops::new_run(p, id);
    p.journal("card.start", Some(id.as_str()), data);
}

// The fetch that reads its base and its head, and the command's answer.
fn opened(mut p: Poster<'_>, id: &CardId, project: &str, word: &str) -> Decision {
    p.fetch(project);
    let mut d = Decision::ok(json!({ word: {"card": id.as_str()} }));
    p.finish(&mut d);
    d
}

/// Enters the train on the fetched branch head and PR destination or project base (fac-111).
/// A branch tip absent from this fetch waits for its own fetch unless the mirror lacks the branch.
pub fn fetched(p: &mut Poster<'_>, c: Card, tip: &Sha, result: &Value, ctx: Ctx<'_>) {
    let Some(branch) = c.branch.clone() else { return };
    let tip_of = |b: &str| result.get("tips").and_then(|t| t.get(b)).and_then(Value::as_str).map(Sha::new);
    let dest = c.review.as_ref().and_then(|r| r.dest.clone());
    let base = dest.as_deref().map_or_else(|| Some(tip.clone()), tip_of);
    let head = c.head.clone().or_else(|| tip_of(&branch));
    let (Some(base), Some(head)) = (base, head) else {
        let missing = if c.head.is_none() && tip_of(&branch).is_none() { branch } else { dest.unwrap_or_default() };
        let all = result.get("branches").and_then(Value::as_array);
        let known = all.is_some_and(|all| all.iter().any(|b| b.as_str() == Some(missing.as_str())));
        if !known {
            let text = format!("{}'s branch {missing} isn't on {}'s origin: close the card", c.id, c.project);
            asks::raise(p, Raise::new(c.id.as_str(), AskKind::Error, Color::Red, text));
        }
        return;
    };
    crate::queue::entries::enter(p, c, base, head, ctx);
}

/// `rereview <card>` (fac-349, fac-352): our card's PR's review card enters the train again on the PR's current
/// head, nothing pushed, an ended one reopened with its rounds, findings and reviewers, once a round or run failed.
pub(super) fn rereview(state: &State, card: &CardId, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let rc = match review_card(state, &id) {
        Ok(rc) => rc,
        Err(words) => return Decision::refused(words),
    };
    let (project, pr) = (rc.project.clone(), rc.pr.unwrap_or_default());
    let mut p = Poster::for_ctx(state, ctx);
    if rc.stage.is_live() {
        let Some(head) = rc.head.clone() else {
            return Decision::refused(format!("{} has no head to review yet", rc.id));
        };
        crate::queue::entries::moved(&mut p, rc.clone(), &head);
    } else {
        let open = state.world.open_prs.get(project.as_str()).and_then(|all| all.iter().find(|x| x.pr == pr));
        let Some(open) = open.cloned() else {
            return Decision::refused(format!("PR {pr} isn't open on {project} as the poller last read it"));
        };
        let r = rc.review.as_deref().cloned().unwrap_or_default();
        let dest = Some(open.dest.clone()).filter(|d| !d.is_empty()).or(r.dest);
        start(&mut p, project.as_str(), &open, Review { auto: r.auto, dest, ..Review::default() });
    }
    p.fetch(project.as_str());
    p.journal("review.again", Some(rc.id.as_str()), json!({"of": id, "pr": pr}));
    let mut d = Decision::ok(json!({"rereview": {"card": id.as_str(), "review": rc.id.as_str()}}));
    p.finish(&mut d);
    d
}

// Our card's PR's review card, if a round or a run of it failed last and none is under way.
fn review_card(state: &State, id: &CardId) -> Result<Card, String> {
    let ours = state.cards.get(id).ok_or_else(|| format!("no card `{id}`"))?;
    if ours.mode == Mode::Review {
        return Err(format!("`rereview` reviews our card's PR again; {id} is a review card"));
    }
    let pr = ours.pr.ok_or_else(|| format!("{id} has no PR: `rereview` reviews a card's PR again"))?;
    let rid = card_id(ours.project.as_str(), pr);
    let rc = state.cards.get(&rid).ok_or_else(|| format!("PR {pr} of {id} has no review card, {rid}"))?;
    if super::gate::subject_of(rc).is_some() {
        return Err(format!("{rid} reviews {id}'s verification now: its next fix asks its next round"));
    }
    let review = rc.review.as_deref();
    if rc.stage.is_live() && rc.paused {
        return Err(format!("{rid} is stopped: `resume {rid}` first"));
    }
    if review.and_then(Review::open).is_some() || rc.landing.is_some() {
        return Err(format!("A round of {rid} is under way: its verdict comes first"));
    }
    let round_failed = review.and_then(Review::last_read).and_then(|r| r.verdict) == Some(Verdict::Failed);
    let run_failed = review.and_then(|r| r.finished.as_ref()).is_some_and(|f| f.failed);
    if !round_failed && !run_failed {
        return Err(format!("{rid}'s last round and its last run didn't fail: there is nothing to run again"));
    }
    Ok(rc.clone())
}

/// Says a review card's news to Andrei, in its room.
pub fn to_andrei(p: &mut Poster<'_>, id: &CardId, text: &str) {
    p.post(id.as_str(), Sender::Engine, Some("andrei"), MessageKind::Engine, "", text);
}
