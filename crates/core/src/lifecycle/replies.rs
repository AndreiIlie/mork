//! What a card agent's reply moves (4.4.2): the specification's recap, the reviews, the report,
//! `blocked`, and the foreman's head.

use mork_types::ask::{AskKind, Color};
use mork_types::card::{Card, Mode, Stage};
use mork_types::reply::{Intent, Outcome, Reply, Role};
use mork_types::state::{Agent, Job, ORCHESTRATOR};

use crate::decide::Ctx;
use crate::messages::asks::{self, Raise};
use crate::messages::places::{self, Route, Who};
use crate::post::Poster;

/// Refuses an ineffective foreman outcome only where the card waits on it and nothing brings another turn.
pub fn check_outcome(
    p: &Poster<'_>,
    who: &Who,
    r: &Reply,
    job: &Job,
    a: &Agent,
    routes: &[Route],
) -> Result<(), String> {
    if crate::agents::children::is_child(a) || matches!(r.outcome, Outcome::Working | Outcome::Blocked) {
        return Ok(());
    }
    let card = places::card_of(p.state, &who.place).and_then(|c| p.card(&c));
    // The challenge this turn read waits for an answer: an outcome that can't give one strands it unless a turn is
    // already owed, as for a queued message, or the reply brings one.
    if r.outcome != Outcome::None
        && card.as_ref().and_then(|c| crate::agents::required::bound(c, &who.key, job)).is_some()
        && !owed_turn(a, job)
        && !brings_turn(p, who, r, routes)
    {
        let (outcome, id) = (r.outcome.as_str(), &who.place);
        return Err(format!("the outcome `{outcome}` has no effect at brief; {id}'s challenge waits for `working`"));
    }
    let Some(card) = card.filter(|c| {
        who.place.as_str() == places::room_of(&c.id, who.role)
            && who.role == super::facts::holder(c)
            && matches!(c.mode, Mode::Full | Mode::Report)
    }) else {
        return Ok(());
    };
    let Some((at, expected)) = waiting_for(&card) else { return Ok(()) };
    if late(&card, who, r, job)
        || effective(p, &card, who, r, job)
        || !crate::agents::stalled::waits_after(p, &card, a, job)
        || brings_turn(p, who, r, routes)
    {
        return Ok(());
    }
    Err(format!("the outcome `{}` has no effect at {at}; {} waits for {expected}", r.outcome.as_str(), card.id))
}

// Its next turn runs whatever this reply says: a message queued for it, or one delivered that the turn didn't take.
fn owed_turn(a: &Agent, job: &Job) -> bool {
    a.owed.is_some() || a.wake_after || job.delivered.iter().any(|s| !a.taken.contains(s))
}

fn waiting_for(card: &Card) -> Option<(&'static str, &'static str)> {
    match card.stage {
        Stage::Brief if card.no_planner && super::mock::baseline(card) => {
            Some(("brief", "`built` to end the baseline step, or a question in the hand's `ask`"))
        }
        Stage::Brief => Some(("brief", "`brief-ready` with a recap, or a question in the hand's `ask`")),
        Stage::Build if super::mock::open(card) => {
            card.no_planner.then_some(("build", "`brief-ready` with a mock offer, or a question in the hand's `ask`"))
        }
        Stage::Build => Some(("build", "`built`")),
        Stage::Report if card.mode == Mode::Report => Some(("report", "`report-ready`")),
        Stage::Starting
        | Stage::Checks
        | Stage::Review
        | Stage::Second
        | Stage::Ready
        | Stage::Merging
        | Stage::Report
        | Stage::Done
        | Stage::Closed => None,
    }
}

fn late(card: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    (r.outcome == Outcome::BriefReady
        && super::brief::holds_spec(who, card)
        && !super::rebrief::read_request(card, job))
        || (r.outcome == Outcome::Built && !super::reopen::read_continuation(card, job))
}

fn effective(p: &Poster<'_>, card: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    match r.outcome {
        Outcome::BriefReady => {
            (super::brief::recaps(who, r, card) && super::rebrief::read_request(card, job))
                || super::mock::clear_offer(p, card, who, r, job)
        }
        Outcome::Built => {
            super::baseline::ends(p, card, who, r, job)
                || (makes(who, card, r) && can_submit(card) && super::reopen::read_continuation(card, job))
        }
        Outcome::ReportReady => card.stage == Stage::Report,
        Outcome::ReviewPass
        | Outcome::ReviewFail
        | Outcome::Working
        | Outcome::Blocked
        | Outcome::Reported
        | Outcome::None => false,
    }
}

fn brings_turn(p: &Poster<'_>, who: &Who, r: &Reply, routes: &[Route]) -> bool {
    if !r.hand.ask.is_empty() || (r.hand.color == Color::Red && !r.hand.withdraw_ask) {
        return true;
    }
    r.messages.iter().zip(routes).any(|(m, route)| {
        let request = m.kind == Some(Intent::Request);
        match route {
            Route::Agent { who: target, .. } => {
                (request || target.key == who.key) && !places::ended(p.state, &target.place)
            }
            Route::Post { to, .. } => request && (to == "orchestrator" || to.starts_with(ORCHESTRATOR)),
            // A delivery failure instead owes its sender a turn to read the notice.
            Route::Unplaced { .. } => request,
        }
    })
}

/// What a reply's shape needs before its messages are posted: whether it raises a gate's ask
/// the agent's own ask joins.
pub fn raises_gate(p: &Poster<'_>, who: &Who, r: &Reply) -> bool {
    let Some(card) = places::card_of(p.state, &who.place).and_then(|c| p.card(&c)) else { return false };
    super::brief::recaps(who, r, &card)
        || matches!((who.role, r.outcome, card.stage), (Role::Foreman, Outcome::ReportReady, Stage::Report))
        || r.outcome == Outcome::Blocked
}

pub(super) fn joined(base: &str, who: &Who, r: &Reply) -> String {
    if r.hand.ask.is_empty() {
        base.to_owned()
    } else {
        format!("{base} · {}: {}", who.role.as_str(), r.hand.ask_words())
    }
}

/// An agent that answers again, not blocked, has carried on: its `blocked` and `error` asks are
/// answered, at the latest now (M5.35).
pub fn carried_on(p: &mut Poster<'_>, who: &Who, r: &Reply) {
    if r.outcome == Outcome::Blocked {
        return;
    }
    let mine: Vec<String> = p
        .all_asks()
        .filter(|a| a.open && matches!(a.kind, AskKind::Blocked | AskKind::Error) && a.agent.as_ref() == Some(&who.key))
        .map(|a| a.id.to_string())
        .collect();
    for id in mine {
        asks::close_one(p, &id);
    }
}

/// What an accepted reply of a card's agent moves.
pub fn outcome(p: &mut Poster<'_>, who: &Who, r: &Reply, job: &Job, ctx: Ctx<'_>) {
    let Some(mut card) = places::card_of(p.state, &who.place).and_then(|c| p.card(&c)) else { return };
    let dm = who.key.as_str().ends_with("/dm");
    if super::mock::replied(p, &card, who, r, job)
        || crate::agents::children::replied(p, &card, who, r, job)
        || super::baseline::built(p, &card, (who, r, job), ctx)
    {
        return;
    }
    if crate::agents::required::replied(p, &mut card, who, r, job, ctx) {
        p.put_card(card.clone());
    }

    match (who.role, r.outcome) {
        (_, Outcome::Blocked) => blocked(p, who, r),
        (_, Outcome::BriefReady)
            if super::brief::holds_spec(who, &card) && !super::rebrief::read_request(&card, job) => {}
        (_, Outcome::BriefReady) if super::brief::recaps(who, r, &card) => {
            if !super::baseline::recap_withheld(p, &card, who, job) {
                super::brief::recapped(p, card.clone(), (who, job), r, ctx);
            }
        }
        (Role::Foreman, Outcome::ReportReady) if card.stage == Stage::Report => {
            let mut raise = Raise::new(
                card.id.as_str(),
                AskKind::Done,
                Color::Yellow,
                joined("the report is ready; /done ends the card", who, r),
            );
            raise.design = asks::marked(p.state, who, r);
            asks::raise(p, raise);
        }
        // A gate round's verdict counts for its round only, as `reviews::verdict` guards (fac-95).
        (Role::Foreman, Outcome::ReviewPass | Outcome::ReviewFail) if !dm && card.mode == Mode::Review => {
            crate::reviews::verdict::verdict(p, card.clone(), r, job, ctx);
        }
        // Guards decide the arms above, so this one stands for every reply that moves no stage: `working`, `built`
        // a review out of turn, or a full card's reply that takes no head (fac-652).
        _ => {}
    }
    if makes(who, &card, r) {
        foreman_replied(p, &card, who, r, job);
    }
}

fn blocked(p: &mut Poster<'_>, who: &Who, r: &Reply) {
    let text = joined(&format!("{} is blocked: {}", who.key, r.hand.headline), who, r);
    let mut raise = Raise::new(&places::asks_room(p.state, who), AskKind::Blocked, Color::Red, text);
    raise.agent = Some(who.key.clone());
    raise.design = asks::marked(p.state, who, r);
    asks::raise(p, raise);
}

fn makes(who: &Who, card: &Card, r: &Reply) -> bool {
    who.role == Role::Foreman
        && who.place.as_str() == card.id.as_str()
        && card.mode == Mode::Full
        && (r.outcome == Outcome::Built || card.stage == Stage::Build)
}

// Only replies after the plan's approval and the mock can submit a build (fac-413); after a reopened mock, only
// those of a turn that read the request to continue it (fac-591).
fn foreman_replied(p: &mut Poster<'_>, card: &Card, who: &Who, r: &Reply, job: &Job) {
    if !can_submit(card) {
        if r.outcome == Outcome::Built {
            crate::messages::requests::tell_agent(
                p,
                &card.id,
                who.role,
                "not yet: the specification first",
                if super::mock::open(card) {
                    "The mock is still open: its approve or visual mode off starts the build."
                } else if super::rebrief::back_at(card) {
                    "The card is back at brief: `built` submits nothing until its new plan's approve asks you to continue the build."
                } else {
                    NOT_STARTED
                },
            );
        }
        return;
    }
    let submits = r.outcome == Outcome::Built && super::reopen::read_continuation(card, job);
    if r.outcome == Outcome::Built && !submits {
        crate::messages::requests::tell_agent(p, &card.id, who.role, "not submitted: the build continues", BEFORE);
    }
    let mut c = p.card(&card.id).unwrap_or(card.clone());
    if submits {
        c.gates.built_pending = true;
        crate::reviews::triage::withdraw(p, &mut c);
    }
    p.put_card(c);
    p.intent(crate::decide::Intent::StartJob {
        purpose: mork_types::work::Purpose::Bundle {
            card: card.id.clone(),
            base_in: None,
            recover: None,
            refresh: None,
        },
    });
}

const BEFORE: &str = "This turn began before the engine's request to continue the build, after the reopened mock or the new plan: its `built` submits nothing. Your next turn reads that request.";

// A full card submits only after its plan is approved and no mock is open.
fn can_submit(card: &Card) -> bool {
    !super::facts::agreeing(card) && !super::mock::open(card) && (card.approval.is_some() || card.mode != Mode::Full)
}

// Why a `built` at the brief started nothing: a card's recap starts its build, not a `built`.
const NOT_STARTED: &str = "The specification isn't agreed yet: `built` starts nothing until its plan is approved. A delegated plan never approves itself: it waits for `approve` from the operator, or its project's orchestrator while its open approve-brief ask is given to it; a visual card's waits for the operator's own.";
