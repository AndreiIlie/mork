use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use factory_types::ProjectId;
use factory_types::protocol::{ErrorBody, ErrorCode};
use serde_json::{Value, json};

use super::Engine;
use crate::sockets::Conn;

pub(super) struct Waiter {
    conn: Arc<Conn>,
    id: u64,
}

// The reserved entries remain at the front of the persisted queue until the writer succeeds.
pub(super) struct Batch {
    entries: usize,
    outcome: Receiver<()>,
}

impl Engine {
    pub(in crate::runtime) fn wait_wakes(
        &mut self,
        conn: &Arc<Conn>,
        id: u64,
        project: &ProjectId,
    ) -> Option<Result<Value, ErrorBody>> {
        if !self.ctx.config.projects.get(project).is_some_and(|p| p.orchestrator_waits) {
            let words = format!("project `{project}` must set `orchestrator_waits = true` to use `wakes`");
            return Some(Err(ErrorBody::new(ErrorCode::Refused, words)));
        }
        // Finishing one waiter must never close another project's connection.
        if self.wakes.waiting.values().any(|w| w.conn.id == conn.id) {
            let words = "this connection already holds a `wakes` request";
            return Some(Err(ErrorBody::new(ErrorCode::Refused, words)));
        }
        let waiter = Waiter { conn: Arc::clone(conn), id };
        if let Some(old) = self.wakes.waiting.insert(project.clone(), waiter) {
            old.conn
                .answer(old.id, Err(ErrorBody::new(ErrorCode::Replaced, "another `wakes` request replaced this one")));
            old.conn.finish();
        }
        self.settle_waiting_wakes();
        self.send_waiting_wake(project);
        None
    }

    pub(in crate::runtime) fn waiting_wakes_closed(&mut self, conn: u64) {
        self.wakes.waiting.retain(|_, waiter| waiter.conn.id != conn);
    }

    pub(super) fn tick_waiting_wakes(&mut self) {
        self.settle_waiting_wakes();
        let projects: Vec<_> = self.wakes.waiting.keys().cloned().collect();
        for project in projects {
            self.send_waiting_wake(&project);
        }
    }

    fn send_waiting_wake(&mut self, project: &ProjectId) {
        if self.wakes.delivering.contains_key(project) {
            return;
        }
        let Some(queue) = self.wakes.queues.get(project).filter(|q| !q.is_empty()) else { return };
        let Some(waiter) = self.wakes.waiting.remove(project) else { return };
        let outcome = waiter.conn.answer_and_finish(waiter.id, json!({"wakes": queue}));
        self.wakes.delivering.insert(project.clone(), Batch { entries: queue.len(), outcome });
    }

    pub(in crate::runtime) fn settle_waiting_wakes(&mut self) {
        let queues = &mut self.wakes.queues;
        let mut changed = false;
        self.wakes.delivering.retain(|project, batch| match batch.outcome.try_recv() {
            Ok(()) => {
                if let Some(queue) = queues.get_mut(project) {
                    queue.drain(..batch.entries);
                    changed = true;
                }
                false
            }
            Err(TryRecvError::Disconnected) => false,
            Err(TryRecvError::Empty) => true,
        });
        if changed {
            self.wakes_keep();
        }
    }
}
