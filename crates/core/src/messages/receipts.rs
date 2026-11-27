//! Read receipts belong to the addressed seat, including when two seats share a challenge place.

use mork_types::JobId;
use mork_types::chat::SeatReceipt;
use mork_types::state::{Agent, AgentStatus, Job, Message, MessageKind, Sender};

use crate::state::State;

pub(crate) struct Recipient<'a> {
    pub agent: &'a Agent,
    pub receipt: SeatReceipt,
    short: &'a str,
}

/// The seats whose addressed messages can reach them from this place.
pub(crate) fn recipients<'a>(state: &'a State, place: &str) -> Vec<Recipient<'a>> {
    let challenge = place.strip_suffix("/challenge");
    let first = super::places::card_of(state, place).and_then(|k| state.cards.get(&k)).map_or(0, |c| c.first_seq);
    state
        .agents
        .values()
        .filter_map(|agent| {
            let key = agent.key.as_str();
            let local = agent.place == place;
            let name = challenge.and_then(|c| key.strip_prefix(c)?.strip_prefix('/'));
            if !local && !matches!(name, Some("foreman" | "planner" | "foreman/dm")) {
                return None;
            }
            let short = if !local && name == Some("foreman/dm") {
                key
            } else {
                key.trim_end_matches("/dm").rsplit('/').next()?
            };
            let job = current_job(state, agent);
            let mut taken = agent.taken.clone();
            if let Some(job) = job
                && let Some(read) = state.world.live_reads.get(&job.id)
            {
                taken.extend(read.iter().filter(|seq| job.delivered.contains(seq)));
            }
            taken.retain(|seq| *seq >= agent.seen);
            taken.sort_unstable();
            taken.dedup();
            let receipt = SeatReceipt {
                recipient: agent.key.clone(),
                seen: agent.seen.max(first),
                taken,
                interruptible: !agent.paused && agent.retiring.is_none() && job.is_some_and(|j| !j.stop_requested),
            };
            Some(Recipient { agent, receipt, short })
        })
        .collect()
}

pub(super) fn current_job<'a>(state: &'a State, a: &Agent) -> Option<&'a Job> {
    let AgentStatus::Working { job } = &a.status else { return None };
    state.jobs.get(job).filter(|j| j.owner == a.key && j.purpose.is_none() && j.settled.is_none())
}

impl Recipient<'_> {
    fn addressed(&self, m: &Message) -> bool {
        let Some(to) = m.to.as_deref() else { return false };
        let key = &self.agent.key;
        let local_dm = to == "dm" && key.as_str() == m.place.as_str() && key.as_str().ends_with("/dm");
        (self.short == to || key.as_str() == to || local_dm)
            && (!matches!(&m.from, Sender::Agent(from) if from == key) || m.kind == MessageKind::Request)
    }
}

fn unique<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let seat = matches.next()?;
    matches.next().is_none().then_some(seat)
}

pub(crate) fn addressed<'a, 's>(seats: &'a [Recipient<'s>], m: &Message) -> Option<&'a Recipient<'s>> {
    unique(seats.iter().filter(|s| s.addressed(m)))
}

pub(crate) fn posted<'a>(state: &'a State, place: &str, seq: u64) -> Option<Recipient<'a>> {
    let m = state.messages_in(place).find(|m| m.seq == seq)?;
    unique(recipients(state, place).into_iter().filter(|s| s.addressed(m)))
}

/// The current job whose stream may already acknowledge this posted message.
pub fn running_recipient(state: &State, place: &str, seq: u64) -> Option<JobId> {
    current_job(state, posted(state, place, seq)?.agent).map(|j| j.id.clone())
}
