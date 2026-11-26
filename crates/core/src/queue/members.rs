//! A card's membership of its merge queue (fac-10, fac-375): verified once its gates pass, it enters at the back; a
//! run that can't be trusted holds it, with a red ask, until `/resume`; it leaves without its candidate.

use mork_types::action::{Action, ActionStatus};
use mork_types::ask::{AskKind, Color};
use mork_types::card::{Card, FixImport, Landing, Stage};
use mork_types::reply::Role;
use mork_types::state::{MessageKind, Sender};
use mork_types::suites::Told;
use mork_types::work::Purpose;
use mork_types::{CardId, Sha};
use serde_json::{Value, json};

use super::candidates::{current, drop_candidate, stale};
use super::order::{ahead, is_train};
use super::train::maintain;
use crate::decide::{Ctx, Decision, Intent};
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

/// `merge <card>`, past the buzz (3.6.2): a card merging is told where its landing stands; one at
/// `ready`, which an own card leaves for its verification when its gates pass, starts it.
pub fn admit(state: &State, card: &str, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card);
    let Some(c) = state.cards.get(&id).cloned() else { return Decision::refused(format!("no card `{card}`")) };
    let mut p = Poster::for_ctx(state, ctx);
    if c.stage == Stage::Merging && c.landing.is_some() {
        return Decision::ok(stands(&p, &id));
    }
    if c.stage == Stage::Done {
        return Decision::ok(json!({"merge": {"card": card, "landing": "landed", "ahead": 0}}));
    }
    let passed = |h: &Sha| c.gates.passed.as_ref() == Some(h);
    if c.head.clone().filter(passed).is_none() {
        return Decision::refused(format!("{card} has no head that passed its checks to merge"));
    }
    if c.stage != Stage::Ready {
        return Decision::refused(format!("{card} isn't ready: it is at {}", c.stage.as_str()));
    }
    enter(&mut p, c, ctx);
    let mut d = Decision::ok(stands(&p, &id));
    p.finish(&mut d);
    d
}

fn stands(p: &Poster<'_>, id: &CardId) -> Value {
    let c = p.card(id);
    let (step, n) = c.as_ref().map_or(("queued", 0), |c| (step(c.landing.as_ref()), ahead(p, c)));
    json!({"merge": {"card": id.as_str(), "landing": step, "ahead": n}})
}

// A landing's step, as answers and messages name it.
fn step(l: Option<&Landing>) -> &'static str {
    match l {
        None => "none",
        Some(Landing::Queued { .. }) => "queued",
        Some(Landing::Fetch { .. }) => "fetch",
        Some(Landing::HandMerged) => "hand-merged",
        Some(Landing::Prepare(_)) => "prepare",
        Some(Landing::Test(_)) => "test",
        Some(Landing::Review(_)) => "review",
        Some(Landing::Passed(_)) => "passed",
        Some(Landing::Published { .. }) => "published",
        Some(Landing::Approval { .. }) => "approval",
        Some(Landing::Merge { .. }) => "merge",
    }
}

/// The headline of the notice that a card's verification started.
pub const VERIFICATION_STARTED: &str = "verification started";

/// An own card whose gates passed is verified on its base, no `/merge` waited for (fac-68, decision 3): its
/// round and its suite, then the back of its project's merge queue, or its PR (fac-375).
pub fn enter(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let (id, project) = (c.id.clone(), c.project.clone());
    c.stage = Stage::Merging;
    c.landing = Some(Landing::Queued { at: p.now });
    p.put_card(c.clone());
    p.journal("merge.queued", Some(id.as_str()), json!({}));
    maintain(p, project.as_str(), ctx);
    let text = format!(
        "{id}'s verification started on {project}'s base: its candidate's checks, then its review round and its whole suite side by side. Once both pass, it enters the merge queue at its back, or its PR opens."
    );
    say(p, &id, VERIFICATION_STARTED, &text);
    // Asked now, the wording is there before the PR opens, so the opening isn't held (fac-296).
    if p.card(&id).is_some_and(|c| crate::board::tickets::unworded(p.state, &c, ctx.config).is_some()) {
        crate::messages::requests::ask_agent(
            p,
            &id,
            Role::Foreman,
            "Write the PR wording",
            &crate::board::tickets::wording_request(
                ctx.config.projects.get(&project).map_or("forge", |p| p.forge.skill()),
            ),
        );
    }
}

/// `resume`'s run of a card's failed job; the train's held run of a candidate still its card's
/// runs again, an older build's run of the queue none (decision 14).
pub fn retry(p: &mut Poster<'_>, card: &CardId, purpose: &Purpose, ctx: Ctx<'_>) {
    let Some(c) = p.card(card) else { return };
    match purpose {
        Purpose::Candidate { .. } if current(&c, purpose) => {
            p.intent(Intent::StartJob { purpose: purpose.clone() });
        }
        Purpose::TrainRun { .. } if current(&c, purpose) => {
            super::leases::start_job(p, purpose.clone(), ctx);
        }
        // A run no longer its card's is no one's: the train runs only its current candidates.
        Purpose::Suite { .. }
        | Purpose::JoinTest { .. }
        | Purpose::Incorporate { .. }
        | Purpose::Final { .. }
        | Purpose::Candidate { .. }
        | Purpose::TrainRun { .. }
        | Purpose::WallTests { .. } => {}
        Purpose::Clone { .. }
        | Purpose::Install { .. }
        | Purpose::Bundle { .. }
        | Purpose::Check { .. }
        | Purpose::Build { .. } => super::leases::start_job(p, purpose.clone(), ctx),
    }
}

/// A card leaves its verification or the merge queue without its candidate or its uncarried landing actions (one left behind would land a head
/// nobody tested: finding 1 of 9bf3c59), and without the train's held run and red ask. Its landing is none after.
pub fn drop_landing(p: &mut Poster<'_>, c: &mut Card) {
    let card = c.id.clone();
    drop_candidate(p, c);
    let left = |a: &Action| matches!(a.status, ActionStatus::Pending | ActionStatus::Held);
    drop_landing_actions(p, &card, "the card left the merge queue", left);
    c.landing = None;
}

/// Drops the card's landing actions `obsolete` picks, with why, and closes the asks tied to them:
/// nothing starts them any more, a resume or recovery neither (fac-98).
pub fn drop_landing_actions(p: &mut Poster<'_>, card: &CardId, words: &str, obsolete: impl Fn(&Action) -> bool) {
    let gone: Vec<Action> = p
        .all_actions()
        .filter(|a| a.what.card() == Some(card) && super::holds::landing_action(&a.what) && obsolete(a))
        .cloned()
        .collect();
    for mut a in gone {
        let id = a.id.clone();
        a.status = ActionStatus::Dropped;
        a.words = Some(words.to_owned());
        a.done_ms = Some(p.now);
        a.cancelled = false;
        crate::board::retry::doubted(&mut a);
        p.journal("action.dropped", Some(card.as_str()), json!({"id": id, "words": words}));
        p.put_action(a);
        let tied: Vec<_> =
            p.all_asks().filter(|x| x.open && x.action.as_ref() == Some(&id)).map(|x| x.id.clone()).collect();
        for x in tied {
            asks::close_one(p, x.as_str());
        }
    }
}

/// Said in the card's room, to the operator, waking nobody.
pub fn say(p: &mut Poster<'_>, id: &mork_types::CardId, headline: &str, text: &str) -> u64 {
    p.post(id.as_str(), Sender::Engine, Some("operator"), MessageKind::Engine, headline, text)
}

/// A new head while the card is merging: its code changed, so it leaves for its gates and is verified again once they
/// pass, entering the queue at its back (decision 10; fac-68, decision 4). Returned at `ready`.
pub fn left_for_head(p: &mut Poster<'_>, mut c: Card, head: &Sha, ctx: Ctx<'_>) -> Card {
    let project = c.project.clone();
    c.stage = Stage::Ready;
    drop_landing(p, &mut c);
    p.put_card(c.clone());
    let text = format!(
        "{} left the merge queue: its head moved to {}. It goes through its gates again, is verified again once they pass, and enters the queue at its back.",
        c.id,
        head.short()
    );
    say(p, &c.id, "left the merge queue: a new head", &text);
    p.journal("merge.left", Some(c.id.as_str()), json!({"why": "head", "head": head}));
    maintain(p, &project, ctx);
    c
}

/// A fix request to the foreman; a card merging leaves its verification or the queue, its commits about
/// to change (decision 10).
pub fn send_back(p: &mut Poster<'_>, mut c: Card, text: &str, ctx: Ctx<'_>) {
    let queued = c.landing.is_some();
    let project = c.project.clone();
    if queued {
        drop_landing(p, &mut c);
    }
    let again = if queued {
        " Once its gates pass again, it is verified again and enters the merge queue at its back."
    } else {
        ""
    };
    crate::lifecycle::fixes::send_back(p, c, &format!("{text}{again}"));
    if queued {
        maintain(p, &project, ctx);
    }
}

/// A verification or queue job that can't be trusted, couldn't run or was cut: the card keeps its place, the queue's
/// front holding the queue, and a red ask says why; `/resume` runs it again (decision 14).
pub fn hold(p: &mut Poster<'_>, mut c: Card, purpose: Purpose, kind: AskKind, why: &str) {
    let (id, project) = (c.id.clone(), c.project.clone());
    c.failed_job = Some(purpose);
    p.put_card(c);
    let text = format!("{id} is held on {project}: {why}. /resume runs it again.");
    p.journal("merge.held", Some(id.as_str()), json!({"why": why}));
    asks::raise(p, Raise::new(id.as_str(), kind, Color::Red, text));
}

/// A train's job ended without its result: failed, or cut (stopped is `resume`'s as ever); one for
/// a candidate given up only removes its clone, and an older build's run counts for nothing.
pub fn job_failed(p: &mut Poster<'_>, c: Card, purpose: &Purpose, words: &str, cut: bool) {
    if !is_train(Some(purpose)) || stale(p, &c, purpose) {
        return;
    }
    let what = if matches!(purpose, Purpose::Candidate { .. }) { "the making of its candidate" } else { "its run" };
    let why = if cut {
        format!("{what} was interrupted")
    } else {
        format!("{what} failed: {}", words.lines().next().unwrap_or(""))
    };
    let kind = if cut { AskKind::Cut } else { AskKind::Error };
    hold(p, c, purpose.clone(), kind, &why);
}

/// Leaves verification or the merge queue without waking any seat, returning the candidate's base tip (fac-680).
pub(crate) fn exit_verification(p: &mut Poster<'_>, c: &mut Card, why: &str) -> Option<Sha> {
    let (id, project) = (c.id.clone(), c.project.clone());
    // The base's tip under its candidate reaches the clone before the request (fac-144).
    let tip = super::order::candidate(c).map(|k| k.on.clone()).or_else(|| super::order::base_on(p, project.as_str()));
    c.stage = Stage::Build;
    c.gates.passed = None;
    drop_landing(p, c);
    asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
    p.journal("merge.left", Some(id.as_str()), json!({"why": why}));
    tip
}

/// Sends the return-from-verification fix request to the card's foreman, importing `tip` first when present.
pub(crate) fn send_return_request(p: &mut Poster<'_>, c: &Card, tip: Option<Sha>, text: &str, ctx: Ctx<'_>) {
    let id = c.id.clone();
    let again = "Once its gates pass again, it is verified again and enters the merge queue at its back.";
    let ask = format!("{text}\n\nSet it right as your role says; it goes through its gates again. {again}");
    let headline = "Returned from verification".to_owned();
    // A failed suite's request names its log's copy in the card's clone (fac-164).
    let told = match tip {
        Some(tip) => Told::Fix {
            card: id.clone(),
            fix: FixImport { tip, base: super::joins::base(c, ctx), headline, text: ask },
        },
        None => Told::Agent { card: id.clone(), role: Role::Foreman, headline, text: ask },
    };
    super::suite_news::send(p, &id, told);
}

/// Out of its verification or the merge queue to the foreman (decision 3).
pub fn leave(p: &mut Poster<'_>, mut c: Card, why: &str, text: &str, ctx: Ctx<'_>) {
    let (id, project) = (c.id.clone(), c.project.clone());
    let tip = exit_verification(p, &mut c, why);
    say(p, &id, "returned to its foreman", &format!("{id} returned to its foreman: {why}."));
    p.put_card(c.clone());
    send_return_request(p, &c, tip, text, ctx);
    maintain(p, &project, ctx);
}
