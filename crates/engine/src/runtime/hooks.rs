// One connection per project reads its stream from after the last line written to its hook (fac-119, fac-244),
// a few at a time so a replay never fills the queue; a project's hook never touches another's.

use std::collections::BTreeMap;
use std::sync::Arc;

use mork_core::decide::Event;
use mork_types::ProjectId;
use mork_types::protocol::{ErrorBody, ErrorCode};
use serde_json::{Value, json};

use super::Engine;
use crate::sockets::Conn;

// Lines queued and not yet written, at most: the connection's queue holds 256.
const AHEAD: u64 = 64;

/// A project's open hook: its connection, the next line to send it, and each line queued with the
/// connection's count of lines then, to tell when it was written.
pub struct Hook {
    conn: Arc<Conn>,
    next: u64,
    queued: Vec<(u64, u64)>,
}

pub type Hooks = BTreeMap<ProjectId, Hook>;

impl Engine {
    // `hook`: replaces the project's open hook, answers with the line it starts after, and sends
    // the lines from there; answered here, but for a refusal.
    pub(super) fn hook(&mut self, conn: &Arc<Conn>, id: u64, project: &ProjectId) -> Option<Result<Value, ErrorBody>> {
        // One hook per connection: replacing one project's must never close another's.
        if self.hooks.values().any(|h| h.conn.id == conn.id) {
            let words = "this connection already holds a hook: hook on a connection of its own".to_owned();
            return Some(Err(ErrorBody::new(ErrorCode::Refused, words)));
        }
        self.hook_settle(project);
        let from = match self.store.delivered(project) {
            Ok(Some(seq)) => seq,
            // The first hook keeps requests still relevant to the orchestrator, rather than old chatter.
            Ok(None) => {
                let now = self.state.orchestrator_next.saturating_sub(1);
                let given: Vec<_> =
                    self.state.asks.values().filter(|a| a.open && a.given).map(|a| a.id.clone()).collect();
                let opinions: Vec<_> =
                    self.state.asks.values().filter(|a| a.open && !a.given).map(|a| a.id.clone()).collect();
                self.store.first_hook(project, now, &given, &opinions).unwrap_or_else(|e| {
                    self.log(&format!("{project}'s orchestrator's first hook couldn't be recorded: {e}"));
                    now
                })
            }
            Err(e) => {
                let words = format!("{project}'s stream couldn't be read: {e}");
                return Some(Err(ErrorBody::new(ErrorCode::Refused, words)));
            }
        };
        let replaced = self.hooks.remove(project).map(|old| {
            old.conn.send_line(&json!({"event": {"orchestrator": "replaced"}}).to_string());
            old.conn.finish();
        });
        conn.answer(id, Ok(json!({"hook": {"project": project, "from": from}})));
        self.hooks.insert(project.clone(), Hook { conn: Arc::clone(conn), next: from + 1, queued: Vec::new() });
        self.hook_send(project);
        if replaced.is_none() {
            self.apply(&Event::Hooked { project: project.clone(), hooked: true });
        }
        None
    }

    pub(super) fn hook_closed(&mut self, conn: u64) {
        let Some(project) = self.hooks.iter().find(|(_, h)| h.conn.id == conn).map(|(p, _)| p.clone()) else {
            return;
        };
        self.hook_settle(&project);
        self.hooks.remove(&project);
        self.apply(&Event::Hooked { project, hooked: false });
    }

    // Sends every open hook the lines it hasn't been sent.
    pub(super) fn hooks_send(&mut self) {
        let projects: Vec<ProjectId> = self.hooks.keys().cloned().collect();
        for p in &projects {
            self.hook_send(p);
        }
    }

    // Sends the project's hook the lines it hasn't been sent, a few ahead of what it was written.
    fn hook_send(&mut self, project: &ProjectId) {
        let Some(h) = self.hooks.get(project) else { return };
        if h.next >= self.state.orchestrator_next {
            return;
        }
        let (queued, written) = h.conn.counts();
        let room = AHEAD.saturating_sub(queued.saturating_sub(written));
        if room == 0 {
            return;
        }
        let lines = match self.store.events_after(project, h.next - 1, room) {
            Ok(l) => l,
            Err(e) => return self.log(&format!("{project}'s stream couldn't be read: {e}")),
        };
        // Fewer than asked: the project has no line before the next one recorded, whichever project's.
        let all = (lines.len() as u64) < room;
        let Some(h) = self.hooks.get_mut(project) else { return };
        for e in lines {
            if !h.conn.send_line(&json!({"event": {"orchestrator": e}}).to_string()) {
                return;
            }
            h.queued.push((h.conn.counts().0, e.seq));
            h.next = e.seq + 1;
        }
        if all {
            h.next = self.state.orchestrator_next;
        }
    }

    // Keeps the last line the project's hook was written, so its next hook starts after it.
    fn hook_settle(&mut self, project: &ProjectId) {
        let Some(h) = self.hooks.get_mut(project) else { return };
        let (_, written) = h.conn.counts();
        let seq = h.queued.iter().take_while(|(n, _)| *n <= written).map(|(_, s)| *s).last();
        h.queued.retain(|(n, _)| *n > written);
        if let Some(seq) = seq
            && let Err(e) = self.store.set_delivered(project, seq)
        {
            self.log(&format!("{project}'s last line written couldn't be recorded: {e}"));
        }
    }

    // Keeps every open hook's last line written.
    pub(super) fn hooks_settle(&mut self) {
        let projects: Vec<ProjectId> = self.hooks.keys().cloned().collect();
        for p in &projects {
            self.hook_settle(p);
        }
    }

    // Each tick: what was written kept, and the next lines sent.
    pub(super) fn hooks_tick(&mut self) {
        self.hooks_settle();
        self.hooks_send();
    }
}
