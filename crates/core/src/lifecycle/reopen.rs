//! The operator's `mock <card>` (fac-591): a visual card's approved mock reopened in the same run, at any stage until its
//! final landing starts; the whole submission withdrawn and the build held until the mock's approve.

use mork_types::CardId;
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::AskKind;
use mork_types::card::{Card, Gates, MockStep, Mode, Rebrief, Stage};
use mork_types::landing::Landing;
use mork_types::state::Job;
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::asks;
use crate::post::Poster;
use crate::state::State;

/// `mock <card>`, past the buzz: reopens the card's approved mock and withdraws its submission.
pub(super) fn reopen(state: &State, card: &CardId, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(c) = state.cards.get(&id).cloned() else { return Decision::refused(format!("no card `{card}`")) };
    if let Err(why) = reopenable(state, &c) {
        return Decision::refused(why);
    }
    let mut p = Poster::for_ctx(state, ctx);
    let was = c.stage;
    let c = withdraw(&mut p, c, Back::Mock, ctx);
    let notes = c.mock.and_then(|m| m.reopened);
    let text = super::mock::request(&c, ctx.config);
    crate::messages::requests::ask_agent(&mut p, &c.id, super::facts::specifier(&c), super::mock::HEADLINE, &text);
    p.journal("mock.reopened", Some(id.as_str()), json!({"from": was.as_str(), "notes": notes}));
    let mut d = Decision::ok(json!({"mock": {"card": id.as_str()}}));
    p.finish(&mut d);
    d
}

/// Why the card's approved mock can't reopen now, or `Ok`: a live visual full card's approved mock, until its final
/// landing starts (a base move or merge dispatched, or its PR's merge asked).
pub fn reopenable(state: &State, c: &Card) -> Result<(), String> {
    let id = &c.id;
    if c.mode != Mode::Full || !c.visual {
        return Err(format!("{id} isn't a visual full card: it has no mock to reopen"));
    }
    if !c.stage.is_live() {
        return Err(format!("{id} has ended: it is {}", c.stage.as_str()));
    }
    if super::rebrief::back_at(c) {
        return Err(format!("{id} is back at brief: its new plan's approve comes first"));
    }
    match c.mock.map(|m| m.step) {
        Some(MockStep::Approved) => {}
        Some(MockStep::Open | MockStep::Baseline) => return Err(format!("{id}'s mock is already open")),
        Some(MockStep::Skipped) | None => return Err(format!("{id} has no approved mock in this run")),
    }
    if landing_started(state, c) {
        return Err(format!("{id}'s final landing has started: its mock can't reopen"));
    }
    Ok(())
}

// Once a base move is started, held or tried, or a PR's merge asked, the call may already have landed it. A local move
// still waiting to start (its turns running) isn't dispatched: the withdrawal drops it with the rest of the landing.
pub(super) fn landing_started(state: &State, c: &Card) -> bool {
    let ours = |card: &CardId| *card == c.id;
    let dispatched = |a: &Action| {
        let tried = matches!(a.status, ActionStatus::Started | ActionStatus::Held)
            || (a.status == ActionStatus::Pending && a.attempts > 0);
        let live = !matches!(a.status, ActionStatus::Done | ActionStatus::Dropped);
        matches!(&a.what, ActionWhat::MoveBase { card, .. } if ours(card) && tried)
            || matches!(&a.what, ActionWhat::Merge { card, .. } if ours(card) && live)
    };
    let pr_merge = matches!(c.landing, Some(Landing::Merge { .. })) && c.pr.is_some();
    let final_step = matches!(c.landing, Some(Landing::HandMerged));
    final_step || pr_merge || state.actions.values().any(dispatched)
}

/// Why a card's submission is withdrawn: the operator reopened its mock, or it went back to brief (fac-638).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Back {
    Mock,
    Brief,
}

impl Back {
    fn why(self, id: &CardId) -> String {
        match self {
            Self::Mock => format!("The operator reopened {id}'s mock"),
            Self::Brief => format!("{id} went back to brief"),
        }
    }
}

/// Every gate of the head goes and the card is back at build, or at brief: a bundle, take, check, review, verification
/// or queue still running for it counts for nothing, and its review's waiting report and findings are never sent.
pub(super) fn withdraw(p: &mut Poster<'_>, mut c: Card, back: Back, ctx: Ctx<'_>) -> Card {
    let (id, project) = (c.id.clone(), c.project.clone());
    let queued = c.landing.is_some();
    // A failed round's report held for the suite goes with the candidate, never sent (fac-375).
    let held = crate::queue::order::candidate(&c).is_some_and(|k| k.round.is_some());
    crate::reviews::triage::withdraw(p, &mut c);
    crate::queue::members::drop_landing(p, &mut c);
    c.gates = Gates { clean: c.gates.clean, ..Gates::default() };
    c.draft = None;
    // The decisions recorded after the withdrawal are numbered above the notes the card has now.
    let notes = notes_of(p.state, &c).len();
    marked(&mut c, back, notes);
    p.put_card(c.clone());
    super::retired::retire(p, &id, ctx);
    asks::close(p, id.as_str(), &[AskKind::ApprovePr, AskKind::Checks, AskKind::Mock]);
    crate::reviews::end::withdrawn(p, &c, held, &back.why(&id), back == Back::Brief, ctx.config);
    if queued {
        let text = format!(
            "{id} left the merge queue: {}. Once its build is submitted again and its gates pass, it is verified again and enters the queue at its back.",
            back.why(&id)
        );
        let (headline, why) = match back {
            Back::Mock => ("left the merge queue: its mock reopened", "mock"),
            Back::Brief => ("left the merge queue: it went back to brief", "brief"),
        };
        crate::queue::members::say(p, &id, headline, &text);
        p.journal("merge.left", Some(id.as_str()), json!({"why": why}));
        crate::queue::train::maintain(p, project.as_str(), ctx);
    }
    p.card(&id).unwrap_or(c)
}

// The card's stage and marker: its mock open again at build, or back at brief with its fix request held for the
// withdrawn submission dropped (fac-638).
fn marked(c: &mut Card, back: Back, notes: usize) {
    match back {
        Back::Mock => {
            c.stage = Stage::Build;
            if let Some(m) = c.mock.as_mut() {
                m.step = MockStep::Open;
                m.offered = false;
                m.reopened = Some(notes);
                m.continued = None;
            }
        }
        Back::Brief => {
            c.stage = Stage::Brief;
            c.recap = false;
            c.challenge = None;
            c.fix_import = None;
            c.rebrief = Some(Rebrief { notes, asked: None, continued: None });
        }
    }
}

/// The reopened mock ended, or the new plan was approved: asks the foreman to continue the build, naming the decisions
/// recorded since, and binds the next submission to that request.
pub(super) fn continued(p: &mut Poster<'_>, mut c: Card, from: usize, back: Back) {
    let id = c.id.clone();
    let all = notes_of(p.state, &c);
    let total = all.len();
    let gone = mork_types::decisions::withdrawn(&all);
    let since: Vec<String> = all
        .iter()
        .zip(gone)
        .enumerate()
        .filter(|(i, (n, g))| {
            !g && mork_types::decisions::number(total, *i) > from && mork_types::decisions::decision_words(n).is_some()
        })
        .map(|(_, (n, _))| n.clone())
        .collect();
    let (event, settled) = match back {
        Back::Mock => (
            "the mock reopened",
            format!(
                "{id}'s reopened mock is settled. Continue the build to its approved plan, the mock's changes and the decisions recorded on its card."
            ),
        ),
        Back::Brief => (
            "it went back to brief",
            format!(
                "{id}'s new plan is approved. Continue the build to it, given whole below, and the decisions recorded on its card: what the old plan built and the new one keeps stays, the rest is changed to it."
            ),
        ),
    };
    let decisions = if since.is_empty() {
        format!("No decision was recorded on the card since {event}.")
    } else {
        format!("The decisions recorded since {event}, newest first:\n\n{}", since.join("\n\n"))
    };
    let plan = c.brief.as_deref().filter(|_| back == Back::Brief).map(|b| format!("\n\nThe new plan:\n\n{b}"));
    let text = format!(
        "{settled}{}\n\n{decisions}\n\n\
         Its earlier submission was withdrawn: submit the finished build with `built` again, from a turn that read this request. In the turn you reply `built`, merge the base's tip, already in the clone as `origin/<base>` and kept current by the engine (the clone has no remote: don't fetch), into the branch, commit, and run the checks and the tests of what it touched.",
        plan.unwrap_or_default()
    );
    let seq = crate::messages::requests::ask_continue(p, &id, &text);
    match back {
        Back::Mock => c.mock.iter_mut().for_each(|m| m.continued = Some(seq)),
        Back::Brief => c.rebrief.iter_mut().for_each(|r| r.continued = Some(seq)),
    }
    p.put_card(c);
    let event = if back == Back::Mock { "mock.continued" } else { "brief.continued" };
    p.journal(event, Some(id.as_str()), json!({"seq": seq, "decisions": since.len()}));
}

fn notes_of(state: &State, c: &Card) -> Vec<String> {
    let board = state.board.get(&c.id);
    let description = board.map_or(&c.description, |b| &b.description);
    crate::board::notes::apart(state, board, description).0
}

/// Whether this turn may submit the build: after a reopened mock (fac-591) or a new plan (fac-638), only one whose news
/// reached the request to continue it. One begun before submits nothing.
pub fn read_continuation(card: &Card, job: &Job) -> bool {
    let read = |seq: u64| job.seen.is_some_and(|seen| seq < seen);
    card.mock.and_then(|m| m.continued).is_none_or(read) && card.rebrief.and_then(|r| r.continued).is_none_or(read)
}
