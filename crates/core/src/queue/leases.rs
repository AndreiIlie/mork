//! Test slots (4.9): leases counted per project, on the machine and per card, held while their connection is, reclaimed
//! within 30 s after a restart or reload; an agent's whole suite is refused (fac-10).

use std::collections::BTreeMap;

use mork_types::config::ProjectConfig;
use mork_types::lease::{Lease, LeaseKind};
use mork_types::reply::Role;
use mork_types::state::Sender;
use mork_types::work::Purpose;
use mork_types::{AgentKey, CardId, JobId};
use serde_json::json;

use crate::decide::{Ctx, Decision, Intent};
use crate::post::Poster;
use crate::state::{Change, State};

/// How long a holder has to reclaim its lease after a restart or a reload (4.9.4).
pub const RECLAIM_MS: i64 = 30_000;

/// Why an agent's suite is refused, with arguments or none (fac-10, fac-65).
pub const WHOLE_SUITE: &str = "only verification and the merge queue run the suite's command, with arguments or none, once the card's checks pass: test what you touched with `mork test --run <command>`, a narrow crate, test or filter";

/// The holder of the engine's own leases.
pub const ENGINE: &str = "engine";

pub(crate) fn live(p: &Poster<'_>) -> BTreeMap<mork_types::LeaseId, Lease> {
    let mut all = p.state.leases.clone();
    for c in &p.other {
        if let Change::PutLease(l) = c {
            all.insert(l.id.clone(), l.clone());
        }
        if let Change::RemoveLease { id } = c {
            all.remove(id);
        }
    }
    all
}

pub(crate) fn no_slot(p: &Poster<'_>, project: &str, card: Option<&CardId>, ctx: Ctx<'_>) -> Option<&'static str> {
    let all = live(p);
    let slots = ctx.config.projects.get(project).map_or(1, |c| c.slots);
    let of_project: Vec<&Lease> = all.values().filter(|l| l.project == project).collect();
    if of_project.iter().any(|l| l.reclaim_by.is_some()) {
        Some("its project's leases are being reclaimed")
    } else if of_project.len() as u64 >= slots {
        Some("its project's slots are taken")
    } else if all.len() as u64 >= ctx.config.mork.max_test_runs {
        Some("the machine's test runs are taken")
    } else if card.is_some()
        && all
            .values()
            .any(|l| l.card.as_ref() == card && p.state.agents.get(&l.holder).is_none_or(|a| !slotless(a.role)))
    {
        Some("its card holds a lease")
    } else {
        None
    }
}

fn new_id(p: &Poster<'_>) -> mork_types::LeaseId {
    let all = live(p);
    let mut n = 0;
    loop {
        let id = if n == 0 { format!("lease-{}", p.now) } else { format!("lease-{}-{n}", p.now) };
        if !all.contains_key(id.as_str()) {
            return id.into();
        }
        n += 1;
    }
}

pub(crate) fn grant(p: &mut Poster<'_>, l: Lease) {
    p.journal(
        "lease.granted",
        l.card.as_ref().map(CardId::as_str),
        json!({"lease": l.id, "holder": l.holder.as_str(), "kind": l.kind}),
    );
    p.other.push(Change::PutLease(l));
}

fn place_of(state: &State, key: &AgentKey) -> Option<(CardId, mork_types::ProjectId)> {
    let agent = state.agents.get(key)?;
    let card = crate::messages::places::card_of(state, &agent.place).and_then(|c| state.cards.get(&c))?;
    Some((card.id.clone(), card.project.clone()))
}

/// `test.lease`: granted when a slot is free, or told to wait (the engine asks again when a slot
/// frees); an agent's suite refused, with arguments or none (fac-10, fac-65).
pub fn lease(state: &State, from: &Sender, kind: &str, command: &[String], ctx: Ctx<'_>) -> Decision {
    let kind = match kind {
        "suite" => LeaseKind::Suite,
        "checks" => LeaseKind::Checks,
        "run" => LeaseKind::Run,
        "file" => LeaseKind::File,
        "walls" => LeaseKind::Walls,
        other => return Decision::refused(format!("kind: `{other}` isn't suite, checks, run, file or walls")),
    };
    // A slot runs in an agent's clone: a sender with none is refused by the state, the same
    // whoever it is, never buzzed (3.6.3, the ruling of ba9503a).
    let Some((key, (card, project))) = agent_of(from).and_then(|k| place_of(state, k).map(|w| (k, w))) else {
        return Decision::refused("no clone to test in");
    };
    // The operator's rule (fac-10, fac-375): only verification and the merge queue run the suite; before, an agent tests
    // only what it touched. Its arguments can't be trusted to narrow it: Mork's ignores them (fac-65).
    if kind == LeaseKind::Suite {
        return Decision::refused(WHOLE_SUITE);
    }
    let walls = if kind == LeaseKind::Walls {
        match super::wall_runs::command(state, key, &card, command, ctx) {
            Ok(argv) => Some(argv),
            Err(why) => return Decision::refused(why),
        }
    } else {
        None
    };
    // A project with no checks: nothing to run, no slot taken (4.7, M5.34).
    let what = (kind == LeaseKind::Checks).then(|| checks_run(state, key, &project, ctx));
    if kind == LeaseKind::Checks && ctx.config.projects.get(&project).is_some_and(|p| p.checks.is_none()) {
        let mut v = json!({"no_checks": format!("{project} has no checks")});
        merge(&mut v, what.as_ref());
        return Decision::ok(v);
    }
    if kind == LeaseKind::File
        && let Some(why) = no_file_run(&project, command, ctx)
    {
        return Decision::refused(why);
    }
    let p = Poster::for_ctx(state, ctx);
    let card_slot = (!state.agents.get(key).is_some_and(|a| slotless(a.role))).then_some(&card);
    if let Some(why) = no_slot(&p, &project, card_slot, ctx) {
        return Decision::ok(json!({"wait": {"why": why}}));
    }
    let id = new_id(&p);
    let l = Lease { id, project, card: Some(card), holder: key.clone(), kind, reclaim_by: None };
    leased(p, l, walls.as_deref().unwrap_or(command), what.as_ref(), ctx)
}

fn leased(mut p: Poster<'_>, l: Lease, command: &[String], what: Option<&serde_json::Value>, ctx: Ctx<'_>) -> Decision {
    let id = l.id.clone();
    let own = ctx.config.mork.project.as_ref() == Some(&l.project);
    let argv = argv(ctx.config.projects.get(&l.project), own, l.kind, command);
    let mut v = json!({"lease": {"id": id, "argv": argv}});
    if let (LeaseKind::Walls, Some(card)) = (l.kind, &l.card) {
        p.intent(Intent::StartJob {
            purpose: Purpose::WallTests { card: card.clone(), seat: l.holder.clone(), lease: id, command: argv },
        });
        v["lease"]["walls"] = true.into();
    }
    grant(&mut p, l);
    merge(&mut v, what);
    let mut d = Decision::ok(v);
    p.finish(&mut d);
    d
}

// A `--file` with nothing to run, or no paths: refused before any slot (fac-154).
fn no_file_run(project: &str, command: &[String], ctx: Ctx<'_>) -> Option<String> {
    if ctx.config.projects.get(project).and_then(|p| p.test_file.as_ref()).is_none() {
        Some(format!(
            "{project} has no one-file command (`test_file` in mork.toml): test what you touched with `mork test --run <program> <args>…`"
        ))
    } else if command.is_empty() {
        Some("`mork test --file` needs the paths of the files to test".to_owned())
    } else {
        None
    }
}

// What a `--checks` run names first (fac-165): its base, the fork for a clone with no `origin/<base>`,
// and the checks' command and folder.
fn checks_run(state: &State, key: &AgentKey, project: &str, ctx: Ctx<'_>) -> serde_json::Value {
    let config = ctx.config.projects.get(project);
    json!({
        "base": config.map(|p| p.base.clone()),
        "fork": fork_of(state, key),
        "checks": config.and_then(|p| p.checks.as_ref()).map(|k| json!({"command": k.command, "folder": k.folder})),
    })
}

// Review clones lack `origin/<base>`: their checks compare against the card's fork or base tip.
fn fork_of(state: &State, key: &AgentKey) -> Option<mork_types::Sha> {
    let agent = state.agents.get(key)?;
    let c = crate::messages::places::card_of(state, &agent.place).and_then(|c| state.cards.get(&c))?;
    c.fork.clone().or(c.base_tip.clone())
}

fn merge(answer: &mut serde_json::Value, what: Option<&serde_json::Value>) {
    if let (Some(a), Some(serde_json::Value::Object(w))) = (answer.as_object_mut(), what) {
        a.extend(w.clone());
    }
}

fn argv(project: Option<&ProjectConfig>, own: bool, kind: LeaseKind, command: &[String]) -> Vec<String> {
    let sh = |cmd: &str| vec!["sh".to_owned(), "-c".to_owned(), format!("{cmd} \"$@\""), "sh".to_owned()];
    match (kind, project) {
        // The engine's checks stage, exactly (gates.rs, plan_check): no paths, the branch point the
        // client reads for `{fork}` (fac-165).
        (LeaseKind::Checks, Some(p)) => {
            // A project with none is answered before a slot (M5.34): `false` is never reached.
            let checks = p.checks.as_ref().map_or_else(|| "false".to_owned(), |c| c.command.clone());
            let mut v = sh(&checks);
            v.extend(["--base".to_owned(), "{fork}".to_owned(), "--json".to_owned(), "{io}/checks.json".to_owned()]);
            v
        }
        // The project's one-file command, the paths appended as the checks' are (fac-154).
        (LeaseKind::File, Some(ProjectConfig { test_file: Some(file), .. })) => {
            let mut v = sh(file);
            v.extend(command.iter().cloned());
            v
        }
        // Mork's own tests, run inside a card's wall (fac-73).
        (LeaseKind::Run, Some(p)) if own => super::own_runs::argv(p, command),
        // An agent's suite is refused before any slot (fac-65).
        (LeaseKind::Run | LeaseKind::Suite | LeaseKind::File | LeaseKind::Walls, _) | (LeaseKind::Checks, None) => {
            command.to_vec()
        }
    }
}

/// `test.resume`: a holder reconnects with its lease id (4.9.4).
pub fn resume(state: &State, from: &Sender, id: &str, ctx: Ctx<'_>) -> Decision {
    let Some(mut l) = state.leases.get(id).cloned() else {
        return Decision::refused("the test slot was lost; run it again");
    };
    if let Some(why) = not_holder(&l, from) {
        return Decision::refused(why);
    }
    if l.kind == LeaseKind::Walls {
        return Decision::refused("a wall-test run cannot be resumed; run it again");
    }
    let mut p = Poster::for_ctx(state, ctx);
    l.reclaim_by = None;
    p.journal("lease.resumed", l.card.as_ref().map(CardId::as_str), json!({"lease": id}));
    p.other.push(Change::PutLease(l));
    let mut d = Decision::ok(json!({"lease": {"id": id}}));
    p.finish(&mut d);
    d
}

/// `test.done`: the run's exit, and the slot freed. No agent's run is remembered as a pass: only
/// verification and the merge queue run the suite (fac-10, fac-375).
pub fn done(state: &State, from: &Sender, id: &str, exit: u64, ctx: Ctx<'_>) -> Decision {
    let Some(l) = state.leases.get(id).cloned() else {
        return Decision::refused("the test slot was lost; run it again");
    };
    if let Some(why) = not_holder(&l, from) {
        return Decision::refused(why);
    }
    if l.kind == LeaseKind::Walls {
        return Decision::refused("the engine frees a wall-test run's slot once its job has ended");
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.journal("lease.done", l.card.as_ref().map(CardId::as_str), json!({"lease": id, "exit": exit}));
    free(&mut p, id, ctx);
    let mut d = Decision::ok(json!({"test.done": {"lease": id}}));
    p.finish(&mut d);
    d
}

fn agent_of(from: &Sender) -> Option<&AgentKey> {
    if let Sender::Agent(k) = from { Some(k) } else { None }
}

// A slot is ended or resumed by the agent that holds it (4.9.3): its own connection's. A lease the sender doesn't hold:
// refused by the state, in the same words whoever sends it (3.6.3, T3.22).
fn not_holder(l: &Lease, from: &Sender) -> Option<String> {
    match agent_of(from) {
        Some(k) if *k == l.holder => None,
        Some(_) | None => Some("that test slot isn't the sender's".to_owned()),
    }
}

pub(crate) fn free(p: &mut Poster<'_>, id: &str, ctx: Ctx<'_>) {
    p.other.push(Change::RemoveLease { id: id.into() });
    p.intent(Intent::SlotFreed);
    start_waiting(p, ctx);
}

/// A client's connection ended: its lease goes with it (4.9.3).
pub fn closed(state: &State, id: &str, ctx: Ctx<'_>) -> Decision {
    let Some(l) = state.leases.get(id) else { return Decision::default() };
    let mut p = Poster::for_ctx(state, ctx);
    p.journal("lease.closed", l.card.as_ref().map(CardId::as_str), json!({"lease": id}));
    if l.kind == LeaseKind::Walls {
        super::wall_runs::stop(&mut p, id, ctx);
    } else {
        free(&mut p, id, ctx);
    }
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// Ordinary agent leases wait 30 s for their holder; connection-bound wall-test runs stop (fac-480).
pub fn started(state: &State, ctx: Ctx<'_>) -> Decision {
    let mut d = Decision::default();
    // The jobs owed before the stop and never recorded run now (M3.9), a clone's imports one at a
    // time (`joins::recover`, fac-97).
    for purpose in
        state.owed_work.values().filter(|w| !super::joins::is_import(w) && !matches!(w, Purpose::WallTests { .. }))
    {
        d.intents.push(crate::decide::Intent::StartJob { purpose: purpose.clone() });
    }
    let mut p = Poster::for_ctx(state, ctx);
    for l in state.leases.values().filter(|l| l.holder.as_str() != ENGINE && l.kind != LeaseKind::Walls) {
        let mut l = l.clone();
        l.reclaim_by = Some(ctx.now + RECLAIM_MS);
        p.other.push(Change::PutLease(l));
    }
    for l in state.leases.values().filter(|l| l.kind == LeaseKind::Walls) {
        super::wall_runs::stop(&mut p, l.id.as_str(), ctx);
    }
    p.finish(&mut d);
    d
}

/// Leases nobody reclaimed in time are freed.
pub fn expire(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let gone: Vec<Lease> = live(p).into_values().filter(|l| l.reclaim_by.is_some_and(|t| t <= p.now)).collect();
    for l in gone {
        p.journal("lease.freed", l.card.as_ref().map(CardId::as_str), json!({"lease": l.id}));
        free(p, &l.id, ctx);
    }
    start_waiting(p, ctx);
}

fn engine_lease(card: &CardId) -> mork_types::LeaseId {
    mork_types::LeaseId::new(format!("{ENGINE}-{card}"))
}

fn engine_run_of(p: &Poster<'_>, card: &CardId) -> bool {
    p.state.jobs.values().any(|j| {
        j.settled.is_none() && p.ending.as_ref() != Some(&j.id) && j.purpose.as_ref().is_some_and(|x| runs_of(x, card))
    })
}

// The engine's test runs of a card, under its one lease.
fn runs_of(x: &Purpose, card: &CardId) -> bool {
    matches!(x, Purpose::Check { card: c, .. } | Purpose::TrainRun { card: c, .. } if c == card)
}

/// Starts a job of no agent's turn; the engine's test runs wait for a slot first (4.9.7).
pub fn start_job(p: &mut Poster<'_>, purpose: Purpose, ctx: Ctx<'_>) {
    let (card, kind) = match &purpose {
        // A clone's imports run one at a time (fac-97).
        Purpose::Bundle { card, base_in: Some(_), recover, .. } => {
            super::joins::start(p, card, true, recover.clone());
            return;
        }
        Purpose::Suite { .. }
        | Purpose::JoinTest { .. }
        | Purpose::Incorporate { .. }
        | Purpose::Final { .. }
        | Purpose::WallTests { .. } => return,
        Purpose::Check { card, .. } => (card.clone(), LeaseKind::Checks),
        // A PR's train checks take no slot, so its round starts while suites hold them (fac-356).
        Purpose::TrainRun { card, checks: true, test: None, .. }
            if p.card(card).is_some_and(|c| super::runs::pr_entry(&c)) =>
        {
            p.intent(Intent::StartJob { purpose });
            return;
        }
        Purpose::TrainRun { card, .. } => (card.clone(), LeaseKind::Suite),
        Purpose::Clone { .. }
        | Purpose::Install { .. }
        | Purpose::Bundle { .. }
        | Purpose::Candidate { .. }
        | Purpose::Build { .. } => {
            p.intent(Intent::StartJob { purpose });
            return;
        }
    };
    let Some(mut c) = p.card(&card) else { return };
    let id = engine_lease(&card);
    let held = live(p).contains_key(&id);
    // One engine run of a card at a time, under its one lease (4.9.2, M3.13): the next waits for
    // the one still running, and starts when its lease is freed.
    if (held && engine_run_of(p, &card)) || (!held && no_slot(p, &c.project, Some(&card), ctx).is_some()) {
        c.gates.slot_wait = Some(purpose);
        p.put_card(c);
        return;
    }
    if !held {
        grant(
            p,
            Lease {
                id,
                project: c.project.clone(),
                card: Some(card),
                holder: AgentKey::new(ENGINE),
                kind,
                reclaim_by: None,
            },
        );
    }
    c.gates.slot_wait = None;
    p.put_card(c);
    p.intent(Intent::StartJob { purpose });
}

// Engine runs waiting for a slot get one, in no promised order.
fn start_waiting(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let waiting: Vec<(CardId, Purpose)> = p
        .state
        .cards
        .keys()
        .filter_map(|id| p.card(id))
        .filter(|c| c.stage.is_live() && !c.paused)
        .filter_map(|c| c.gates.slot_wait.clone().map(|w| (c.id.clone(), w)))
        .collect();
    for (id, purpose) in waiting {
        // A project with no checks takes no slot: its checks pass at once, as `start_checks` has them.
        if let (Purpose::Check { .. }, Some(mut c)) = (&purpose, p.card(&id))
            && !crate::lifecycle::gates::has_checks(&c, ctx)
        {
            c.gates.slot_wait = None;
            crate::lifecycle::gates::start_checks(p, c, ctx);
            continue;
        }
        let project = p.card(&id).map(|c| c.project).unwrap_or_default();
        if no_slot(p, &project, Some(&id), ctx).is_none() {
            start_job(p, purpose, ctx);
        }
    }
}

/// A recorded or owed job ended: its lease goes with it, unless another run of the card holds it
/// now (a newer head's checks, started while a stale one ran: R5).
pub fn job_done(p: &mut Poster<'_>, job: Option<&JobId>, purpose: &Purpose, ctx: Ctx<'_>) {
    let card = match purpose {
        Purpose::Check { card, .. } | Purpose::TrainRun { card, .. } => card,
        Purpose::Clone { .. }
        | Purpose::Install { .. }
        | Purpose::Bundle { .. }
        | Purpose::Suite { .. }
        | Purpose::JoinTest { .. }
        | Purpose::Incorporate { .. }
        | Purpose::Final { .. }
        | Purpose::Candidate { .. }
        | Purpose::WallTests { .. }
        | Purpose::Build { .. } => return,
    };
    let id = engine_lease(card);
    // Another engine run of the card holds the lease on: one running, or one owed and not yet recorded, this decision's
    // included (a review card's suite, started as its checks end: M4.20).
    let of_card = |x: &Purpose| runs_of(x, card);
    let running = p
        .state
        .jobs
        .values()
        .any(|j| Some(&j.id) != job && j.settled.is_none() && j.purpose.as_ref().is_some_and(of_card));
    let owed = p.state.owed_work.iter().any(|(key, w)| {
        of_card(w) && !p.other.iter().any(|c| matches!(c, Change::RemoveOwedWork { key: k } if k == key))
    }) || p.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose: w } if of_card(w)));
    let other = running || owed;
    if !other && live(p).contains_key(&id) {
        p.journal("lease.done", Some(card.as_str()), json!({"lease": id}));
        free(p, &id, ctx);
    }
}

// A researcher's run, or a baseline seat's (fac-636), takes no card slot: neither writes the card's code.
fn slotless(role: Role) -> bool {
    matches!(role, Role::Researcher | Role::Baseline)
}
