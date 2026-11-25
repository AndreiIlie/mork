// A lease is held while the connection that took it is open (4.9); a `test.lease` that must wait is told so
// (`waiting`), and answered once a slot frees. `core` counts and decides; this keeps the connections.

use std::collections::BTreeMap;
use std::sync::Arc;

use factory_core::decide::Event;
use factory_types::Command;
use factory_types::protocol::ErrorBody;
use factory_types::state::Sender;
use serde_json::Value;

use super::Engine;
use crate::sockets::Conn;

/// A `test.lease` waiting for a slot.
#[derive(Debug)]
pub struct Parked {
    pub conn: Arc<Conn>,
    pub id: u64,
    pub cmd: Command,
    pub from: Sender,
}

/// The connections holding leases, and the calls that wait.
#[derive(Debug, Default)]
pub struct Slots {
    /// Lease id → the connection holding it.
    pub held: BTreeMap<factory_types::LeaseId, u64>,
    pub parked: Vec<Parked>,
    /// Wall-test requests receive their one answer when the supervised job ends.
    pub walls: BTreeMap<factory_types::LeaseId, (Arc<Conn>, u64)>,
    /// A slot freed: the waiting calls are asked again.
    pub freed: bool,
}

impl Engine {
    /// `test.lease`, `test.resume`, `test.done`: decided by `core`; the engine notes which
    /// connection holds what, and parks a call that waits. `None` when it waits.
    pub(super) fn slot_request(
        &mut self,
        conn: &Arc<Conn>,
        id: u64,
        cmd: Command,
        from: Sender,
    ) -> Option<Result<Value, ErrorBody>> {
        let d = self.apply(&Event::Command { from: from.clone(), cmd: cmd.clone() });
        let answer = d.answer.unwrap_or(Ok(Value::Null));
        if let Ok(v) = &answer {
            if let Some(wait) = v.get("wait") {
                // No `id` at the top: a client of an older build skips it as no answer.
                conn.send_line(&serde_json::json!({"waiting": {"id": id, "why": wait["why"]}}).to_string());
                self.pipe.slots.parked.push(Parked { conn: Arc::clone(conn), id, cmd, from });
                return None;
            }
            if let Some(lease) = v.pointer("/lease/id").and_then(Value::as_str) {
                self.pipe.slots.held.insert(lease.into(), conn.id);
                if v.pointer("/lease/walls").and_then(Value::as_bool) == Some(true) {
                    self.pipe.slots.walls.insert(lease.into(), (Arc::clone(conn), id));
                    return None;
                }
            }
            if let (Command::TestLease { .. }, Sender::Agent(key)) = (&cmd, &from)
                && v.get("checks").is_some_and(Value::is_object)
            {
                let mut v = v.clone();
                v["checks"]["at"] = self.folder_of(key);
                return Some(Ok(v));
            }
            if let Command::TestDone { lease, .. } = &cmd {
                self.pipe.slots.held.remove(lease);
            }
        }
        Some(answer)
    }

    // The checks folder's commit as the factory reads it on the host, as the gate credits it (4.7.7).
    fn folder_of(&self, key: &factory_types::AgentKey) -> Value {
        let card =
            self.state.agents.get(key).and_then(|a| factory_core::messages::places::card_of(&self.state, &a.place));
        let (commit, dirty) = card.map(|c| self.checks_folder(&c)).unwrap_or_default();
        serde_json::json!({"commit": commit, "dirty": dirty})
    }

    /// A connection closed: the leases it held are freed, and its waiting calls dropped.
    pub(super) fn conn_closed(&mut self, conn: u64) {
        self.pipe.slots.parked.retain(|p| p.conn.id != conn);
        let gone: Vec<factory_types::LeaseId> =
            self.pipe.slots.held.iter().filter(|(_, c)| **c == conn).map(|(l, _)| l.clone()).collect();
        for lease in gone {
            self.pipe.slots.held.remove(&lease);
            self.pipe.slots.walls.remove(&lease);
            self.apply(&Event::LeaseClosed { lease });
        }
    }

    /// After a slot freed, the waiting calls are asked again; any free slot may go to any of
    /// them (4.9.2).
    pub(super) fn slots_tick(&mut self) {
        self.wall_runs_tick();
        if !std::mem::take(&mut self.pipe.slots.freed) {
            return;
        }
        let waiting = std::mem::take(&mut self.pipe.slots.parked);
        for p in waiting {
            if !p.conn.is_open() {
                continue;
            }
            let conn = Arc::clone(&p.conn);
            if let Some(answer) = self.slot_request(&conn, p.id, p.cmd, p.from) {
                conn.answer(p.id, answer);
            }
        }
    }
}
