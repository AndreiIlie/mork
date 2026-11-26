use std::collections::VecDeque;
use std::time::Instant;

use mork_core::door::queue::Waiting;
use mork_types::ActionId;
use mork_types::action::ActionStatus;
use mork_types::door_plan::Envelope;
use serde_json::json;

use super::super::{Engine, Phase};
use crate::clock;

#[derive(Debug, Default)]
pub(super) struct Target {
    running: Option<Running>,
    waiting: VecDeque<ActionId>,
}

#[derive(Debug, Clone)]
pub(super) struct Running {
    id: ActionId,
    requester: String,
    op: String,
    since: Instant,
    timeout_ms: u64,
}

impl Running {
    pub(super) fn facts(&self, ahead: usize) -> Waiting {
        Waiting {
            action: self.id.clone(),
            requester: self.requester.clone(),
            op: self.op.clone(),
            elapsed_ms: u64::try_from(clock::elapsed(self.since).as_millis()).unwrap_or(u64::MAX),
            timeout_ms: self.timeout_ms,
            ahead,
        }
    }
}

fn order(id: &ActionId) -> u64 {
    id.strip_prefix("act-").and_then(|n| n.parse().ok()).unwrap_or(u64::MAX)
}

impl Engine {
    pub(in super::super) fn queue_door(&mut self, id: &str) -> bool {
        let Some(a) = self.state.actions.get(id) else { return false };
        let Some((call, _)) = mork_core::door::parts(a) else { return false };
        if a.status != ActionStatus::Pending || a.cancelled {
            return true;
        }
        let target = call.target.clone();
        let id = a.id.clone();
        let q = self.door.targets.entry(target.clone()).or_default();
        if !q.waiting.contains(&id) {
            let at = q.waiting.iter().position(|x| order(x) > order(&id)).unwrap_or(q.waiting.len());
            q.waiting.insert(at, id.clone());
        }
        self.door_waiting_line(&id);
        self.door_drain(&target);
        true
    }

    pub(super) fn door_behind(&self, id: &ActionId) -> Option<(Running, usize)> {
        let (call, _) = self.state.actions.get(id).and_then(mork_core::door::parts)?;
        let q = self.door.targets.get(&call.target)?;
        let running = q.running.clone()?;
        let ahead = self.door_ahead(id)?;
        Some((running, ahead))
    }

    pub(super) fn door_ahead(&self, id: &ActionId) -> Option<usize> {
        let (call, _) = self.state.actions.get(id).and_then(mork_core::door::parts)?;
        self.door.targets.get(&call.target)?.waiting.iter().position(|x| x == id)
    }

    pub(super) fn door_waiting_line(&mut self, id: &ActionId) {
        let Some((running, ahead)) = self.door_behind(id) else { return };
        let facts = running.facts(ahead);
        let why = mork_core::door::queue::waiting(&facts);
        let mut told = false;
        for w in self.door.waiters.get_mut(id).into_iter().flatten() {
            if w.behind.as_ref().is_some_and(|(r, _)| r.id == running.id) {
                continue;
            }
            w.conn.send_line(&json!({"waiting": {"id": w.req, "why": why}}).to_string());
            w.behind = Some((running.clone(), ahead));
            told = true;
        }
        if told && let Some((call, _)) = self.state.actions.get(id).and_then(mork_core::door::parts) {
            self.door_step(&call.from.clone(), format!("door: {why}"));
        }
    }

    pub(super) fn door_unqueue(&mut self, id: &ActionId) {
        for q in self.door.targets.values_mut() {
            q.waiting.retain(|x| x != id);
        }
    }

    fn door_expire_target(&mut self, target: &str) {
        let ids = self.door.targets.get(target).map(|q| q.waiting.clone()).unwrap_or_default();
        for id in ids {
            self.door_expire(&id);
        }
    }

    fn door_drain(&mut self, target: &str) {
        self.door_expire_target(target);
        if matches!(self.phase, Phase::Reloading { .. })
            || self.door.targets.get(target).is_some_and(|q| q.running.is_some())
        {
            return;
        }
        while let Some(id) = self.door.targets.get(target).and_then(|q| q.waiting.front()).cloned() {
            self.door_expire(&id);
            if let Some(q) = self.door.targets.get_mut(target) {
                q.waiting.retain(|x| x != &id);
            }
            let Some(a) = self.state.actions.get(&id).filter(|a| a.status == ActionStatus::Pending && !a.cancelled)
            else {
                continue;
            };
            let Some((call, _)) = mork_core::door::parts(a) else { continue };
            let timeout_ms = serde_json::from_str::<Envelope>(&call.plan).map(|e| e.bounds.timeout_ms).unwrap_or(0);
            let mut running = Running {
                id: id.clone(),
                requester: call.from.label(),
                op: call.op.clone(),
                since: clock::instant(),
                timeout_ms,
            };
            self.start_action(&id);
            if !self.pipe.in_flight.contains(&id) {
                continue;
            }
            running.since = clock::instant();
            if let Some(q) = self.door.targets.get_mut(target) {
                q.running = Some(running);
            }
            let ids = self.door.targets.get(target).map(|q| q.waiting.clone()).unwrap_or_default();
            for id in ids {
                self.door_waiting_line(&id);
            }
            return;
        }
        self.door.targets.remove(target);
    }

    pub(in super::super) fn door_returned(&mut self, id: &ActionId) {
        let target = self
            .door
            .targets
            .iter()
            .find(|(_, q)| q.running.as_ref().is_some_and(|r| &r.id == id))
            .map(|(t, _)| t.clone());
        let Some(target) = target else { return };
        self.door_expire_target(&target);
        if let Some(q) = self.door.targets.get_mut(&target) {
            q.running = None;
        }
        self.door_drain(&target);
    }

    pub(in super::super) fn door_tick(&mut self) {
        let targets: Vec<_> = self.door.targets.keys().cloned().collect();
        for target in targets {
            self.door_drain(&target);
        }
    }
}
