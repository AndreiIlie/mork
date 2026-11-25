// Smooth delivery (fac-389): a message handed to its recipient's running turn, read between its tool calls;
// the turn's end says whether it was, and one missed wakes its agent then.

use factory_types::state::{Job, JobStatus, Sender};
use factory_types::{AgentKey, JobId};

use super::Poster;
use crate::decide::Intent;
use crate::messages::places::Who;
use crate::state::Change;

impl Poster<'_> {
    /// Hands message `seq`, posted in this decision, to `who`'s running live turn: whether it did. Only Andrei's, an
    /// orchestrator's or its own foreman's go, to an agent neither paused nor ending, whose turn isn't stopping.
    pub fn deliver(&mut self, who: &Who, from: &Sender, seq: u64) -> bool {
        let Some(a) = self.agent(&who.key) else { return false };
        let parent = matches!(from, Sender::Agent(k) if a.parent.as_ref() == Some(k));
        if !(parent || *from == Sender::Andrei || from.is_orchestrator()) {
            return false;
        }
        self.deliver_to(who, from, seq)
    }

    /// Hands the engine's report `seq` of `child`'s ended turn to its own foreman's live turn. Ordinary messages
    /// from a child must use `deliver`.
    pub fn deliver_report(&mut self, who: &Who, child: &AgentKey, seq: u64) -> bool {
        let parent = self.agent(child).and_then(|a| a.parent);
        parent.as_ref() == Some(&who.key) && self.deliver_to(who, &Sender::Agent(child.clone()), seq)
    }

    fn deliver_to(&mut self, who: &Who, from: &Sender, seq: u64) -> bool {
        let Some(a) = self.agent(&who.key) else { return false };
        if a.paused || a.retiring.is_some() {
            return false;
        }
        let Some(mut job) = self.live_job(&who.key) else { return false };
        if !self.messages.iter().any(|m| m.seq == seq) {
            return false;
        }
        // Its answer settled first: a delegated answer is said so where it arrives.
        self.answered(who, from);
        let Some(m) = self.messages.iter().find(|m| m.seq == seq) else { return true };
        let text = crate::agents::prompt::delivered(m);
        // Named in its turn's `io/` as its inputs are: no output of the turn, and a retry's to carry.
        let images: Vec<(String, String)> = m
            .images
            .iter()
            .enumerate()
            .map(|(n, src)| {
                (format!("images/m{seq}-{}-{}", n + 1, src.rsplit('/').next().unwrap_or_default()), src.clone())
            })
            .collect();
        job.io_copies.extend(images.iter().cloned());
        job.delivered.push(seq);
        self.intent(Intent::Deliver { job: job.id.clone(), seq, text, images });
        self.journal(
            "deliver",
            None,
            serde_json::json!({"agent": who.key.as_str(), "job": job.id.as_str(), "seq": seq}),
        );
        self.other.push(Change::PutJob(Box::new(job)));
        true
    }

    // The agent's turn under way, as this decision has it so far.
    fn live_job(&self, key: &AgentKey) -> Option<Job> {
        let changed = |id: &JobId| {
            self.other
                .iter()
                .rev()
                .find_map(|c| if let Change::PutJob(j) = c { Some(j) } else { None }.filter(|j| &j.id == id))
        };
        let job = self.state.jobs.values().find(|j| {
            &j.owner == key && j.purpose.is_none() && j.settled.is_none() && self.ending.as_ref() != Some(&j.id)
        })?;
        let job = changed(&job.id).map_or_else(|| job.clone(), |j| (**j).clone());
        let running = matches!(job.status, JobStatus::Launching | JobStatus::Running);
        (running && !job.stop_requested && job.settled.is_none() && job.spec.live.is_some()).then_some(job)
    }
}
