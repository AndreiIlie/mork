// `started` is committed before each call (2.5); one call at a time per project, on a thread of its own,
// a board write on the engine's. After a crash `pending` ones run and `started` ones are checked first.

use std::collections::BTreeMap;
use std::sync::Arc;

use mork_core::decide::{Decision, Event};
use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::card::Stage;

use super::act_run::Call;
use super::{Engine, Input};
use crate::clock;
use crate::lock::Lock;

/// Locks that keep a project's actions one at a time (a fetch racing another into a mirror).
#[derive(Debug, Default, Clone)]
pub struct Locks(Arc<Lock<BTreeMap<String, Arc<Lock<()>>>>>);

impl Locks {
    pub(super) fn of(&self, key: &str) -> Arc<Lock<()>> {
        Arc::clone(self.0.lock().entry(key.to_owned()).or_default())
    }
}

fn board_write(what: &ActionWhat) -> bool {
    matches!(what, ActionWhat::CreateBoardCard { .. } | ActionWhat::ChangeBoardCard { .. })
}

// The project an action belongs to, for its lock.
pub(super) fn project_of(e: &Engine, what: &ActionWhat) -> mork_types::ProjectId {
    match what {
        ActionWhat::Fetch { project, .. }
        | ActionWhat::OpenPr { project, .. }
        | ActionWhat::BackupPush { project, .. }
        | ActionWhat::MoveBase { project, .. }
        | ActionWhat::CreateBoardCard { project, .. }
        | ActionWhat::ChangeBoardCard { project, .. } => project.clone(),
        ActionWhat::Take { card, .. }
        | ActionWhat::TakeJoin { card, .. }
        | ActionWhat::Push { card, .. }
        | ActionWhat::Comment { card, .. }
        | ActionWhat::ReplyComment { card, .. }
        | ActionWhat::ResolveComment { card, .. }
        | ActionWhat::Merge { card, .. }
        | ActionWhat::RemoveClone { card, .. }
        | ActionWhat::RemoveTrain { card, .. } => {
            e.state.cards.get(card).map(|c| c.project.clone()).unwrap_or_default()
        }
        // A door call's lock is its target's: one call at a time per target (fac-1).
        ActionWhat::Door(c) => mork_types::ProjectId::new(format!("door:{}", c.target)),
    }
}

// The repository counts too, not the PR's number alone (R18, M4.13); each with the comment's id
// when its result kept one, as a finding raised again has its earlier raising's (fac-111).
fn alike_done<'a>(actions: impl Iterator<Item = &'a Action>, action: &Action) -> Vec<Option<u64>> {
    let done = actions.filter(|a| a.id != action.id && a.status == ActionStatus::Done);
    let result =
        |a: &Action, field: &str| a.result.as_ref().and_then(|r| r.get(field)).and_then(serde_json::Value::as_u64);
    match &action.what {
        ActionWhat::Comment { pr, body, repo, .. } => done
            .filter(|a| matches!(&a.what, ActionWhat::Comment { pr: p, body: b, repo: r, .. } if p == pr && b == body && r == repo))
            .map(|a| result(a, "id"))
            .collect(),
        // The PRs other openings made on this branch: a new card's opening is never an earlier one's
        // (fac-382).
        ActionWhat::OpenPr { project, branch, .. } => done
            .filter(|a| matches!(&a.what, ActionWhat::OpenPr { project: p, branch: b, .. } if p == project && b == branch))
            .map(|a| result(a, "pr"))
            .collect(),
        _ => Vec::new(),
    }
}

impl Engine {
    fn call_for(&self, id: &str) -> Option<Call> {
        let action = self.state.actions.get(id)?.clone();
        let card = action.what.card().and_then(|c| self.state.cards.get(c)).cloned();
        let project = project_of(self, &action.what);
        let (heads, branches) = if matches!(action.what, ActionWhat::Fetch { .. }) {
            let heads = self
                .state
                .cards
                .values()
                .filter(|c| c.project == project && c.stage.is_live() && c.stage != Stage::Starting)
                // A card in the train lands its candidate, which the base must be in (fac-68).
                .filter_map(|c| mork_core::queue::order::commit_of(c).map(|h| (c.id.clone(), h)))
                .collect();
            (heads, mork_core::reviews::gate::branches(&self.state, project.as_str()))
        } else {
            (Vec::new(), Vec::new())
        };
        // A comment's earlier ones on the same PR, of cards ended since, are in the store (fac-336).
        let mut stored = mork_core::state::unloaded::alike_stored(&self.state, &action.what);
        // A PR opening's earlier ones on the branch, under the old key or the new (fac-382).
        if let Some(prefix) = action.what.legacy_key() {
            let rest = |r: &str| r.is_empty() || r.starts_with([':', '@']);
            stored.extend(self.state.unloaded.matching(&prefix, rest));
        }
        let stored = if stored.is_empty() { Vec::new() } else { self.store.actions_of(&stored).unwrap_or_default() };
        let same_done = alike_done(self.state.actions.values().chain(&stored), &action);
        Some(Call {
            home: self.opts.home.clone(),
            config: self.ctx.config.clone(),
            telemetry: self.ctx.telemetry.clone(),
            action,
            card,
            heads,
            branches,
            same_done,
            now: clock::now_ms(),
            verdict: self.pipe.verdict.of(&project),
            readers: self.pipe.readers.clone(),
            tx: self.tx.clone(),
            project,
            bin: self.opts.bin.clone(),
            stop: self.door.stops.get(id).cloned().unwrap_or_default(),
        })
    }

    // Kept to check after a crash (2.5.2).
    fn prepare(&self, id: &str) -> Option<serde_json::Value> {
        let action = self.state.actions.get(id)?;
        if let ActionWhat::Door(c) = &action.what {
            return Some(self.door_prepared(c));
        }
        // Only a change the engine decides (a file the boards' read doesn't hold): `core` decided
        // the others (M3.16).
        let ActionWhat::ChangeBoardCard { project, file, edit, after, .. } = &action.what else { return None };
        if action.result.is_some() || !after.is_empty() {
            return None;
        }
        use crate::board::BoardRead;
        let config = &self.ctx.config.projects.get(project)?.board;
        crate::board::read(config).prepare(file, edit, clock::now_ms())
    }

    /// Runs a pending action: `started` first, then the call on a thread, a board write's on this
    /// one. One that reaches Bitbucket waits for this start's first read of who the token is (M3.2).
    pub(super) fn run_action(&mut self, id: &str) {
        // A reload under way starts no outward work: pending reads drop at the next start, and
        // other pending actions run then (1.8.6.3, M5.17, fac-632).
        if matches!(self.phase, super::Phase::Reloading { .. }) {
            return;
        }
        if self.wait_to_remove(id) {
            return;
        }
        // Each board write is prepared from the file the one before it left (fac-318).
        let what = self.state.actions.get(id).map(|a| &a.what);
        if what.is_some_and(board_write) {
            return self.run_action_now(id);
        }
        if let Some(action) = self.state.actions.get(id).filter(|a| super::act_run::reaches_forge(&self.ctx.config, &a.what)) {
            let project = project_of(self, &action.what);
            if !self.pipe.token_asked.contains(&project) {
                self.pipe.token_wait.entry(project).or_default().push(id.into());
                return;
            }
        }
        if self.queue_door(id) {
            return;
        }
        self.start_action(id);
    }

    // Door dispatch reaches here only with its target free (fac-505).
    pub(super) fn start_action(&mut self, id: &str) {
        let prepared = self.prepare(id);
        let d = self.apply(&Event::ActionStarted { id: id.into(), prepared });
        // Its guard in the start may have dropped it (a door call, fac-1): then no call.
        if d.changes.is_empty() || self.state.actions.get(id).is_none_or(|a| a.status != ActionStatus::Started) {
            return;
        }
        // A door call's stop, set when its last waiting caller goes (fac-183).
        if self.state.actions.get(id).is_some_and(|a| matches!(a.what, ActionWhat::Door(_))) {
            self.door.stops.entry(id.into()).or_default();
        }
        let Some(call) = self.call_for(id) else { return };
        let lock = self.pipe.locks.of(&project_of(self, &call.action.what));
        let tx = self.tx.clone();
        let id = mork_types::ActionId::new(id);
        self.pipe.in_flight.insert(id.clone());
        std::thread::spawn(move || {
            let _held = lock.lock();
            let result = super::act_run::run(&call);
            let _ = tx.send(Input::Event(Box::new(Event::ActionReturned { id, result })));
        });
    }

    /// Runs a pending action on this thread, start to record: a board write, a command's
    /// answered with what was written (4.18).
    pub(super) fn run_action_now(&mut self, id: &str) {
        self.pipe.later.retain(|i| !matches!(i, mork_core::decide::Intent::RunAction { id: x } if x == id));
        let prepared = self.prepare(id);
        let d = self.apply(&Event::ActionStarted { id: id.into(), prepared });
        if d.changes.is_empty() || self.state.actions.get(id).is_none_or(|a| a.status != ActionStatus::Started) {
            return;
        }
        let Some(call) = self.call_for(id) else { return };
        let result = super::act_run::run(&call);
        self.event_in(&Event::ActionReturned { id: id.into(), result });
    }

    // Before anything is recorded: any field, then what the write would hold, checked as the write checks
    // it; the boards are read first, so it is decided from the files as they are now.
    fn board_decided(
        &mut self,
        cmd: &mork_types::Command,
        from: &mork_types::state::Sender,
    ) -> Result<(Decision, i64), String> {
        let secrets = crate::secrets::Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        if let Some(words) = super::act_board::command_leak(&secrets, cmd) {
            return Err(words);
        }
        self.read_boards_now();
        let now = clock::now_ms();
        let d = self.decide(&Event::Command { from: from.clone(), cmd: cmd.clone() }, now);
        let leak = d.changes.iter().find_map(|c| match c {
            mork_core::state::Change::PutAction(a) => super::act_board::write_leak(&secrets, &a.what, cmd),
            _ => None,
        });
        leak.map_or(Ok((d, now)), Err)
    }

    /// A board command: decided, its write carried out at once, and answered with what was
    /// written, or refused with nothing written.
    pub(super) fn board_request(
        &mut self,
        cmd: mork_types::Command,
        from: mork_types::state::Sender,
    ) -> Result<serde_json::Value, mork_types::protocol::ErrorBody> {
        use mork_types::protocol::{ErrorBody, ErrorCode};
        // The decision checked is the one committed: nothing is decided between them.
        let (d, now) = self.board_decided(&cmd, &from).map_err(|words| ErrorBody::new(ErrorCode::Leak, words))?;
        let d = self.commit_decided(d, now);
        let answer = d.answer.unwrap_or_else(|| Ok(serde_json::json!({})));
        let Ok(v) = &answer else { return answer };
        let Some(id) = v
            .as_object()
            .and_then(|o| o.values().next())
            .and_then(|x| x.get("action"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            return answer;
        };
        self.run_action_now(&id);
        let Some(a) = self.state.actions.get(id.as_str()) else { return answer };
        match a.status {
            ActionStatus::Done => {
                let mut out = v.clone();
                if let (Some(o), Some(r)) = (
                    out.as_object_mut().and_then(|o| o.values_mut().next()).and_then(|x| x.as_object_mut()),
                    a.result.as_ref().and_then(|r| r.as_object()),
                ) {
                    o.extend(r.clone());
                }
                Ok(out)
            }
            ActionStatus::Held | ActionStatus::Dropped => {
                let words = a.words.clone().unwrap_or_else(|| "the board write was refused".into());
                let code = if words.starts_with("leak") {
                    ErrorCode::Leak
                } else if words.starts_with("internals") {
                    ErrorCode::Internals
                } else {
                    ErrorCode::Refused
                };
                Err(ErrorBody::new(code, words))
            }
            ActionStatus::Pending | ActionStatus::Started => answer,
        }
    }

    /// Bitbucket was asked who the token is: what waited for it runs, under the verdict now in
    /// the state (its guard, 2.5.8).
    pub(super) fn token_asked(&mut self, project: mork_types::ProjectId) {
        self.pipe.token_asked.insert(project.clone());
        for id in self.pipe.token_wait.remove(&project).unwrap_or_default() {
            if self.state.actions.get(id.as_str()).is_some_and(|a| a.status == ActionStatus::Pending) {
                self.run_action(id.as_str());
            }
        }
    }

    /// An event a thread brought back: an action's answer is recorded here (2.5.6).
    pub(super) fn event_in(&mut self, event: &Event) {
        self.apply(event);
        if let Event::ActionReturned { id, .. } = event {
            self.pipe.in_flight.remove(id);
            self.door_returned(id);
        }
        // A board written: the index is read again at once, so the next write is made from the
        // file as it now is (M3.16); a card's writes made together read it once, after the last (fac-563).
        if let Event::ActionReturned { id, .. } | Event::ActionFound { id, .. } = event
            && self.state.actions.get(id.as_str()).is_some_and(|a| board_write(&a.what))
            && !self.more_writes(id.as_str())
        {
            self.read_boards_now();
        }
    }

    // Another board write of the same card's still to run, whose return reads the boards: a board command reads
    // them before it decides anyway.
    fn more_writes(&self, id: &str) -> bool {
        let Some(card) = self.state.actions.get(id).and_then(|a| a.what.card()) else { return false };
        self.state.actions.values().any(|a| {
            a.id.as_str() != id
                && a.status == ActionStatus::Pending
                && !a.cancelled
                && board_write(&a.what)
                && a.what.card() == Some(card)
        })
    }

    /// After a start (2.5.2): each `started` action is checked on a thread of its own under its project's lock,
    /// and what was found applied, all before the engine takes a command (2.7, step 7; M5.16).
    pub(super) fn check_started(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut out = 0;
        for a in self.state.actions.values().filter(|a| !a.cancelled && a.status == ActionStatus::Started) {
            let Some(call) = self.call_for(&a.id) else { continue };
            let lock = self.pipe.locks.of(&project_of(self, &call.action.what));
            let (tx, id) = (tx.clone(), a.id.clone());
            out += 1;
            std::thread::spawn(move || {
                let _held = lock.lock();
                let found = super::act_found::found(&call);
                let _ = tx.send(Event::ActionFound { id, found });
            });
        }
        drop(tx);
        // A SIGTERM ends the wait: the checks left out find their actions `started` at the next
        // start (M5.31).
        let mut back = 0;
        while back < out && !mork_sys::term_requested() {
            match rx.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(e) => {
                    back += 1;
                    self.apply(&e);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Recovery has ended (2.7, step 9): the `pending` actions run.
    pub(super) fn run_pending(&mut self) {
        let mut pending: Vec<_> = self
            .state
            .actions
            .values()
            .filter(|a| !a.cancelled && a.status == ActionStatus::Pending)
            .map(|a| a.id.clone())
            .collect();
        // Creation order, as ids sort as text (act-10 before act-9) and a withdrawal precedes its decision (fac-318).
        pending.sort_by_key(|id| (id.strip_prefix("act-").and_then(|n| n.parse::<u64>().ok()), id.clone()));
        for id in pending {
            self.run_action(&id);
        }
    }
}
