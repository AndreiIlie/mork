//! A foreman's finished children wake it once (fac-528): a completion report reaching it outside a turn while another
//! of its children is at work is held, and the held reports wake it when the last child at work stops.

use mork_types::AgentKey;
use mork_types::reply::Role;
use mork_types::state::{Agent, AgentStatus, HeldReports, Owed, Sender};

use crate::messages::places::{self, Who};
use crate::post::Poster;

/// Holds `child`'s completion report, of review round `round` or none, for its foreman `to` while another of its
/// children is at work: whether it did. A planner's researchers' reports are never held.
pub(super) fn hold(p: &mut Poster<'_>, to: &Who, child: &AgentKey, round: Option<u32>) -> bool {
    let Some(mut f) = p.agent(&to.key).filter(|_| to.role == Role::Foreman) else { return false };
    // A turn its live delivery missed takes the report at its end, and a wake already owed carries it (fac-389).
    if in_turn(p, &f) || waking(&f) || !children(p, &to.key).iter().any(|a| a.key != *child && at_work(a, p.now)) {
        return false;
    }
    // As a wake merges its causes: the round stands only while every held report is that round's (fac-277).
    let round = match f.held_reports {
        None => round,
        Some(h) => round.filter(|n| h.round == Some(*n)),
    };
    f.held_reports = Some(HeldReports { round, waking: false });
    p.put_agent(f);
    true
}

/// Each foreman among `touched` or their parents whose held reports no child at work is left to follow wakes for
/// them once, as their completion would have (fac-314), and again if it may run with that wake lost before launching.
pub(crate) fn release(p: &mut Poster<'_>, touched: &[AgentKey]) {
    let mut foremen: Vec<AgentKey> =
        touched.iter().filter_map(|k| p.agent(k)).flat_map(|a| [a.parent, Some(a.key)]).flatten().collect();
    foremen.sort();
    foremen.dedup();
    for key in foremen {
        let Some(f) = p.remaining_agent(&key).filter(|a| a.held_reports.is_some()) else { continue };
        if !children(p, &key).iter().any(|a| at_work(a, p.now)) {
            release_one(p, f);
        }
    }
}

// The reports stay held until a turn launched with its news reads them (`turns::prepared`), or a verdict or
// `report-ready` drops their wake (`children::drop_wake`); a card no longer live owes it to nobody.
fn release_one(p: &mut Poster<'_>, mut f: Agent) {
    let (Some(held), Some(who)) = (f.held_reports, places::who_of(p.state, &f.key)) else { return };
    let live = places::card_of(p.state, &f.place).and_then(|c| p.card(&c)).is_none_or(|c| c.stage.is_live());
    // A stop, a hold or a second cut waits for its resume, whose replayed turn ends before the news.
    let stays = f.paused || matches!(f.status, AgentStatus::Error { .. } | AgentStatus::Cut | AgentStatus::Stopped);
    if live && held.waking && (waking(&f) || stays) {
        return;
    }
    f.held_reports = live.then_some(HeldReports { waking: true, ..held });
    p.put_agent(f);
    if live {
        p.wake_for_report(&who, &Sender::Engine, held.round);
    }
}

// A wake owed now or after the retry, correction or closing turn it owes.
fn waking(a: &Agent) -> bool {
    matches!(a.owed, Some(Owed::Wake)) || a.wake_after
}

// In a turn: one running, or settling in another decision than this.
fn in_turn(p: &Poster<'_>, f: &Agent) -> bool {
    matches!(f.status, AgentStatus::Working { .. }) || super::closing::running(p, &f.key)
}

// At work: a turn of its runs, its closing turn aside, or one owed it may start as the scheduler counts time
// (`turns::due`), unpaused.
fn at_work(a: &Agent, now: i64) -> bool {
    if matches!(a.status, AgentStatus::Working { .. }) {
        !super::closing::asked(a)
    } else {
        a.retiring.is_none() && super::turns::ready_for_turn(a, now)
    }
}

// A foreman's numbered children as this decision has them, any it removed gone: its planner holds no report back
// (fac-633).
fn children(p: &Poster<'_>, foreman: &AgentKey) -> Vec<Agent> {
    p.agents_where(|a| a.parent.as_ref() == Some(foreman) && super::children::is_child(a))
}
