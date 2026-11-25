// Door requests resolve against current targets and dispatch or raise their once/no ask.
// Waiting callers get their answer; asked and held calls owe news when no caller waits (fac-124, fac-632).

mod queue;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use factory_core::decide::Event;
use factory_types::action::ActionStatus;
use factory_types::door::{Op, OpError, canonical, is_read, status};
use factory_types::door_call::{DoorCall, Verdict};
use factory_types::protocol::{ErrorBody, ErrorCode};
use factory_types::state::Sender;
use factory_types::{ActionId, JobId};
use serde_json::{Value, json};

use super::{Engine, Input};
use crate::sockets::Conn;
use crate::{clock, door, random};

/// What the door keeps in memory.
#[derive(Debug, Default)]
pub struct DoorRt {
    waiters: BTreeMap<ActionId, Vec<Waiter>>,
    /// A started call's stop, set once its last waiting request's connection closed (fac-183).
    pub stops: BTreeMap<ActionId, Arc<AtomicBool>>,
    targets: BTreeMap<String, queue::Target>,
}

#[derive(Debug)]
struct Waiter {
    conn: Arc<Conn>,
    req: u64,
    asked: Instant,
    behind: Option<(queue::Running, usize)>,
}

/// The kill of a read's query its server didn't take, tried again as the same query was asked
/// again (fac-184): whether the query has ended, and the request waiting on it.
pub struct Killed {
    pub id: ActionId,
    pub ended: bool,
    pub conn: Arc<Conn>,
    pub req: u64,
    pub call: DoorCall,
    pub validator: Verdict,
    pub asked: Instant,
}

/// A `door` request as it came: what was asked, and who asked from where.
pub struct Asked {
    pub target: String,
    pub op: String,
    pub params: Value,
    pub from: Sender,
    pub job: Option<JobId>,
}

impl Engine {
    pub(super) fn door_request(&mut self, conn: &Arc<Conn>, req: u64, a: Asked) -> Option<Result<Value, ErrorBody>> {
        let asked = clock::instant();
        let op = match Op::parse(&a.op, &a.params) {
            Ok(op) => op,
            Err(OpError::Unsupported(why)) => return Some(self.door_refused(&a, status::UNSUPPORTED, &why)),
            Err(OpError::Invalid(why)) => return Some(Err(ErrorBody::new(ErrorCode::Invalid, why))),
        };
        let resolution = door::read(&self.opts.home)
            .map_err(|e| format!("the door's targets.toml can't be used: {e}"))
            .and_then(|f| door::resolve(&f, &a.target, &op));
        let res = match resolution {
            Ok(r) => r,
            Err(why) => return Some(self.door_refused(&a, status::NOT_ALLOWED, &why)),
        };
        let (place, card) = self.door_place(&a.from);
        let call = DoorCall {
            from: a.from.clone(),
            place,
            card,
            turn: self.logical_turn(a.job.as_ref()),
            job: a.job,
            target: a.target.clone(),
            op: a.op.clone(),
            params: canonical(&op.params()),
            plan: res.plan,
            nonce: random::uuid(),
        };
        let stored = factory_core::state::unloaded::alike_stored(
            &self.state,
            &factory_types::action::ActionWhat::Door(Box::new(call.clone())),
        );
        self.load(&factory_core::state::unloaded::Wanted { actions: stored, ..Default::default() });
        // The same query, its read before maybe still running on its server: its kill is tried
        // again on a thread of its own (it reaches the server), and core decides after (fac-184).
        if let Some((id, _)) = factory_core::door::reads::running(&self.state, &call, clock::now_ms())
            && let Some(action) = self.state.actions.get(&id).cloned()
        {
            let (tx, home, bin) = (self.tx.clone(), self.opts.home.clone(), self.opts.bin.clone());
            let (conn, validator) = (Arc::clone(conn), res.resolved.verdict);
            std::thread::spawn(move || {
                let ended = door::call::kill_again(&home, &bin, &action);
                let _ = tx.send(Input::DoorKilled(Box::new(Killed { id, ended, conn, req, call, validator, asked })));
            });
            return None;
        }
        self.door_ask(conn, req, call, res.resolved.verdict, asked)
    }

    // Its kill tried again: core settles whether its read still runs, then decides the request.
    // A request whose caller went meanwhile is dropped: nothing would wait on it, or stop it.
    pub(super) fn door_killed(&mut self, k: Killed) {
        self.apply(&Event::DoorKilled { id: k.id, ended: k.ended });
        if !k.conn.is_open() {
            return;
        }
        if let Some(answer) = self.door_ask(&k.conn, k.req, k.call, k.validator, k.asked) {
            k.conn.answer(k.req, answer);
        }
    }

    fn door_ask(
        &mut self,
        conn: &Arc<Conn>,
        req: u64,
        call: DoorCall,
        validator: Verdict,
        asked: Instant,
    ) -> Option<Result<Value, ErrorBody>> {
        let (from, step) = (call.from.clone(), format!("door: {} on {}", call.op, call.target));
        // A call whose caller went is being stopped: a request asked again runs anew, never waits on it.
        let stopped =
            self.door.stops.iter().filter(|(_, s)| s.load(Ordering::SeqCst)).map(|(id, _)| id.clone()).collect();
        let d = self.apply(&Event::DoorAsked { call: Box::new(call), validator, stopped });
        let answer = d.answer.unwrap_or_else(|| Ok(json!({})));
        self.door_step(&from, step);
        let Ok(v) = &answer else { return Some(answer) };
        let id = v.pointer("/door/action").and_then(Value::as_str).map(ActionId::new);
        let under_way = v.pointer("/door/status").and_then(Value::as_str) == Some(status::UNDER_WAY);
        match id {
            Some(id) if under_way => {
                self.door.waiters.entry(id.clone()).or_default().push(Waiter {
                    conn: Arc::clone(conn),
                    req,
                    asked,
                    behind: None,
                });
                self.door_waiting_line(&id);
                None
            }
            _ => Some(answer),
        }
    }

    fn door_refused(&mut self, a: &Asked, word: &str, why: &str) -> Result<Value, ErrorBody> {
        let e = Event::DoorRefused {
            from: a.from.clone(),
            target: a.target.clone(),
            op: a.op.clone(),
            status: word.to_owned(),
            why: why.to_owned(),
        };
        self.apply(&e).answer.unwrap_or_else(|| Ok(json!({"door": {"status": word, "why": why}})))
    }

    fn door_place(&self, from: &Sender) -> (factory_types::PlaceId, Option<factory_types::CardId>) {
        let place = match from {
            Sender::Agent(k) => self.state.agents.get(k).map_or_else(|| "main".into(), |a| a.place.clone()),
            Sender::Andrei | Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => "main".into(),
        };
        let card = factory_core::messages::places::card_of(&self.state, &place);
        (place, card)
    }

    // The first of the corrections and cut retries a job continues: the dedup scope.
    fn logical_turn(&self, job: Option<&JobId>) -> Option<JobId> {
        let mut at = job?.clone();
        for _ in 0..64 {
            let j = self.state.jobs.get(&at);
            let before = j
                .and_then(|j| j.corrects.clone())
                .or_else(|| self.state.jobs.values().find(|x| x.retry_of.as_ref() == Some(&at)).map(|x| x.id.clone()));
            match before {
                Some(b) => at = b,
                None => break,
            }
        }
        Some(at)
    }

    // Read here: `targets.toml` is in no wall, and the description names no connection.
    pub(super) fn door_help(&self, target: Option<&str>) -> Result<Value, ErrorBody> {
        let f = door::read(&self.opts.home).map_err(|e| ErrorBody::new(ErrorCode::Refused, e))?;
        let v = match target {
            Some(id) => {
                let t =
                    f.targets.get(id).ok_or_else(|| ErrorBody::new(ErrorCode::Refused, format!("no target `{id}`")))?;
                factory_core::door::target::describe(&f, id, t)
            }
            None => factory_core::door::target::describe_all(&f),
        };
        Ok(json!({"door.help": v}))
    }

    // The start compares this with the plan and verdict the call was checked on.
    pub(super) fn door_prepared(&self, c: &DoorCall) -> Value {
        match door::current(&self.opts.home, c) {
            Ok(r) => json!({"plan": r.plan, "verdict": r.resolved.verdict}),
            Err(why) => json!({"why": format!("its target can't be resolved now: {why}")}),
        }
    }

    // Waiting requests are answered once it isn't under way; asked and held calls owe news with none.
    pub(super) fn door_settle(&mut self, id: &ActionId) {
        let Some(a) = self.state.actions.get(id).cloned() else { return };
        let Some((call, _)) = factory_core::door::parts(&a).map(|(c, d)| (c.clone(), d.clone())) else { return };
        let word = factory_core::door::status_of(&a);
        if word == status::UNDER_WAY {
            return;
        }
        self.door_unqueue(id);
        self.door.stops.remove(id);
        let waiting: Vec<Waiter> =
            self.door.waiters.remove(id).unwrap_or_default().into_iter().filter(|w| w.conn.is_open()).collect();
        let answer = factory_core::door::answer(&a);
        for w in &waiting {
            w.conn.answer(w.req, Ok(answer.clone()));
        }
        let told = a.door.as_ref().and_then(|d| d.told.clone());
        let step = format!("door: {} on {}: {word}", call.op, call.target);
        if !waiting.is_empty() && told.as_deref() != Some(word.as_str()) {
            self.door_step(&call.from, step);
            self.apply(&Event::DoorDelivered { id: id.clone(), answered: true });
        } else if factory_core::door::owed_news(&a) {
            self.door_step(&call.from, step);
            self.apply(&Event::DoorDelivered { id: id.clone(), answered: false });
        }
    }

    // Unstarted reads have nothing left to do for a vanished caller; a started SQL read still
    // needs its stop flag so its server's session is killed (fac-183, fac-505).
    pub(super) fn door_closed(&mut self, conn: u64) {
        let ids: Vec<_> = self.door.waiters.keys().cloned().collect();
        for id in ids {
            let Some(waiting) = self.door.waiters.get_mut(&id) else { continue };
            waiting.retain(|w| w.conn.id != conn && w.conn.is_open());
            if waiting.is_empty() {
                self.door.waiters.remove(&id);
                self.door_caller_gone(&id);
            }
        }
    }

    fn door_caller_gone(&mut self, id: &ActionId) {
        let Some(a) = self.state.actions.get(id) else { return };
        let Some((c, _)) = factory_core::door::parts(a) else { return };
        if a.status == ActionStatus::Pending && is_read(&c.op) {
            self.apply(&Event::DoorUnstarted { id: id.clone(), busy: None });
        } else if a.status == ActionStatus::Started && c.op == "sql.select" {
            self.door.stops.entry(id.clone()).or_default().store(true, Ordering::SeqCst);
        }
    }

    fn door_expire(&mut self, id: &ActionId) {
        let Some(a) = self.state.actions.get(id).filter(|a| a.status == ActionStatus::Pending) else { return };
        let Some((call, _)) = factory_core::door::parts(a) else { return };
        let from = call.from.clone();
        let Some(waiting) = self.door.waiters.remove(id) else { return };
        let (due, live): (Vec<_>, Vec<_>) = waiting
            .into_iter()
            .filter(|w| w.conn.is_open())
            .partition(|w| clock::elapsed(w.asked) >= Duration::from_secs(60));
        if due.is_empty() {
            if live.is_empty() {
                self.door_caller_gone(id);
            } else {
                self.door.waiters.insert(id.clone(), live);
            }
            return;
        }
        let behind = self.door_behind(id).or_else(|| due.first().and_then(|w| w.behind.clone()));
        let facts = behind.map(|(r, _)| r.facts(self.door_ahead(id).unwrap_or(0)));
        let answer = factory_core::door::queue::busy(id, facts.as_ref());
        let why = answer.pointer("/door/why").and_then(Value::as_str).unwrap_or_default().to_owned();
        if live.is_empty() {
            self.door.waiters.insert(id.clone(), due);
            self.apply(&Event::DoorUnstarted { id: id.clone(), busy: Some(why) });
        } else {
            self.door.waiters.insert(id.clone(), live);
            for w in due {
                w.conn.answer(w.req, Ok(answer.clone()));
            }
            self.door_step(&from, format!("door: busy: {why}"));
        }
    }

    // Pending reads lost their callers across restart; asked and held outcomes still owe news (fac-632).
    pub(super) fn door_recover(&mut self) {
        let reads: Vec<ActionId> = self
            .state
            .actions
            .values()
            .filter(|a| {
                a.status == ActionStatus::Pending && factory_core::door::parts(a).is_some_and(|(c, _)| is_read(&c.op))
            })
            .map(|a| a.id.clone())
            .collect();
        for id in reads {
            self.apply(&Event::DoorUnstarted { id, busy: None });
        }
        let owed: Vec<ActionId> =
            self.state.actions.values().filter(|a| factory_core::door::owed_news(a)).map(|a| a.id.clone()).collect();
        for id in owed {
            self.apply(&Event::DoorDelivered { id, answered: false });
        }
    }

    fn door_step(&mut self, from: &Sender, words: String) {
        if let Sender::Agent(k) = from {
            self.seen.entry(k.clone()).or_default().steps.push(words);
            self.views_cache.stale = true;
        }
    }
}
