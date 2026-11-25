// Who settles a full card's specification and where its agreement stands, said from the state at every turn: a fresh
// session can't recall it, and a resumed one is told what moved (fac-16, fac-616).

use std::fmt::Write as _;

use factory_types::card::{Card, Mode};
use factory_types::reply::Role;
use factory_types::state::Agent;

use super::card_of;
use super::held::Sections;
use crate::state::State;

pub(super) const FOREMAN_SPECIFICATION: &str = "On a full card the planner settles specification questions. Send those questions to `planner` as requests, including any finding you hold contradicts the plan, and any words from Andrei that change the specification. Raise your other asks to Andrei in your hand.";

// How the agent holding a card's specification agrees it with Andrei (fac-16): the procedure is
// the Factory manual's; this says that the agent holds it.
const SPECIFICATION: &str = "## The specification\n\nYou hold this card's specification. Read the card and scout with researchers as the `agreeing-a-specification` skill says. Recap the plan with `brief-ready`, its sketch in the hand's `changes` and `code_shape`.";

const CHALLENGED_RECAPS: &str = "Each recap is challenged by the card's `foreman` before it is offered. Settle its objections with it, by messages as long as it takes, and recap once settled: that recap is challenged again. Your messages to `foreman` and its to you pass in the card's challenge place, which no window shows; any disagreement left unsettled opens as your ask (given to the project's orchestrator when delegated). Approval ends your work at brief and hands the build to that foreman.";

const WITHDRAWN: &str = "A recap waiting for `approve` is withdrawn by Andrei's words in your room, the card's project's orchestrator's words while its open plan ask is given to it (unless answering your unanswered mechanical request), an `answer` ask you raise, your hand's `ask` or a red headline raised as one; a reply raising none, a `blocked` one included, leaves it waiting. A new recap replaces it and is offered under the card's current delegation.";

const UNCHALLENGED_RECAPS: &str = "This card has no planner, and no seat challenges your recap: it is offered once made. Write, edit and commit nothing before approval. Approval releases your brief researchers, and the build request comes to you in this session.";

const UNCHALLENGED_VISUAL_RECAPS: &str = "This card has no planner, and no seat challenges your recap: it is offered once made. Edit no tracked repository files before approval: only `.git/factory-mock/` is yours to edit at brief. Approval releases your brief researchers, and the build request comes to you in this session.";

const ALONE_BASELINE: &str = "\n## The baseline\n\nThis visual card is at its baseline step. Read the card and the code, name the pages the card changes, and spawn its baseline seat (`factory ctl spawn <card> baseline '<task>'`, naming no model or effort) to reproduce them as they stand today in `.git/factory-mock/`. Write nothing yourself. Once its report says the baseline is done, end `built` to release the seat and move on to agreeing the specification and changing the mock; with no existing page to reproduce, spawn nothing and end `built` at once.\n";

const DISPUTE: &str = "\n## The specification\n\nFor this dispute, read the card and its recorded decisions. Never decide a specification question alone: raise any open question in your hand's `ask` under the current delegation level, record each settled decision and answer whoever asked. Leave the card at its current stage.\n";

const ALONE_AFTER: &str = "\n## The specification\n\nThis card has no planner: you hold its specification after approval too. Never decide a specification question alone: raise any open question in your hand's `ask` under the current delegation level, including a finding that contradicts the plan or words of Andrei's that change it; record each settled decision and answer whoever asked. Leave the card at its current stage.\n";

/// Whether an agent is a full card's foreman under a planner, which settles its specification questions.
pub(super) fn planner_foreman(state: &State, a: &Agent) -> bool {
    a.role == Role::Foreman && card_of(state, a).is_some_and(|c| c.mode == Mode::Full && !c.no_planner)
}

// A no-planner card's room foreman holds its specification throughout (fac-616).
fn alone(a: &Agent, c: &Card) -> bool {
    a.role == Role::Foreman && c.mode == Mode::Full && c.no_planner && a.place.as_str() == c.id.as_str()
}

/// The specification's section for the seat holding it, or the open mock's standing; its rule only when changed.
pub(super) fn specification(state: &State, a: &Agent, s: &mut Sections<'_>, out: &mut String) {
    let Some(c) = card_of(state, a) else { return };
    let alone = alone(a, c);
    if crate::lifecycle::mock::open(c) && matches!(a.role, Role::Foreman | Role::Planner) {
        return crate::lifecycle::mock::standing(c, a.role == Role::Planner || alone, s, out);
    }
    if a.role != Role::Planner && !alone {
        return;
    }
    if !crate::lifecycle::facts::agreeing(c) {
        let text = if alone { ALONE_AFTER } else { DISPUTE };
        if s.sends("specification", text) {
            out.push_str(text);
        }
        return;
    }
    if alone && crate::lifecycle::mock::baseline(c) {
        if s.sends("specification", ALONE_BASELINE) {
            out.push_str(ALONE_BASELINE);
        }
        return;
    }
    let mut rule = String::new();
    if !c.delegated {
        rule.push_str("\nThis card is not delegated: settle specification questions with Andrei and wait for the plan's approve.\n");
    }
    if c.visual {
        rule.push_str("\nThis card is visual: its plan, its mock included, waits for Andrei's own approve, whatever its delegation; it is never given to an orchestrator.\n");
    }
    let recaps = match (alone, c.visual) {
        (true, true) => UNCHALLENGED_VISUAL_RECAPS,
        (true, false) => UNCHALLENGED_RECAPS,
        (false, _) => CHALLENGED_RECAPS,
    };
    let _ = write!(rule, "\n{SPECIFICATION} {recaps} {WITHDRAWN}\n\n");
    out.push_str(if s.sends("specification", &rule) { &rule } else { "\n## The specification\n\n" });
    out.push_str("Where it stands: ");
    stands(state, a, c, (alone, s), out);
    if crate::lifecycle::mock::briefing(c) {
        crate::lifecycle::mock::at_brief(c, s, out);
    }
}

// The standing draft is written unless the session holds it, as its own recap or an earlier turn's prompt (fac-623).
fn stands(state: &State, a: &Agent, c: &Card, (alone, s): (bool, &mut Sections<'_>), out: &mut String) {
    let draft = c.draft.as_deref().map(str::trim).filter(|d| s.sends("plan", d));
    let (below, plan) = draft.map_or(("", String::new()), |d| (", below,", format!("\n\n{d}")));
    match (&c.draft, c.recap) {
        (Some(_), true) if c.visual => {
            let _ = writeln!(out, "your recap{below}, the plan and its mock, waits for Andrei's own `approve`.{plan}");
        }
        (Some(_), true) => {
            let _ = writeln!(
                out,
                "your recap{below} waits for `approve` from Andrei, or its project's orchestrator while its open plan ask is given to it.{plan}"
            );
        }
        (Some(_), false) => {
            let now = if alone {
                format!("your recap{below} is offered at once: no seat challenges it")
            } else if c.challenge.is_some() {
                "the foreman owes its challenge of this recap; no approve is offered yet".to_owned()
            } else {
                "settle the foreman's objections with it; raise one it still holds in your hand's `ask` with both positions, one at a time (never decide it alone); recap once settled".to_owned()
            };
            let _ = writeln!(out, "{now}.{plan}");
        }
        (None, _) => {
            let me = factory_types::state::Sender::Agent(a.key.clone());
            let last = state
                .messages_in(a.place.as_str())
                .filter(|m| m.seq >= c.first_seq && m.from == me)
                .filter_map(|m| m.hand.as_deref().filter(|h| !h.ask.is_empty()))
                .last();
            match last {
                Some(h) => {
                    let _ = writeln!(out, "no recap waits. The last question you elevated: {}", h.ask_words());
                }
                None => out.push_str("no recap waits, and you have elevated no question yet.\n"),
            }
        }
    }
}
