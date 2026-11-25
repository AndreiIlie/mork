// live-1's foreman runs on Codex, live-2's is picked onto Claude (fac-119); 1.11, 1-4.

use std::time::Duration;

use factory_core::state::State;
use factory_engine::jobs::folder::JobDir;
use factory_types::AgentKey;
use factory_types::job::WallMode;
use factory_types::state::{Job, JobStatus, Settlement};
use serde_json::json;

use super::Report;
use super::scratch::{Scratch, alive, interrupted};

/// A turn's agent: its key, its place, the name that reaches it there, and the CLI it runs.
pub struct Leg {
    pub key: &'static str,
    pub place: &'static str,
    pub to: &'static str,
    pub cli: &'static str,
}

/// Claude's turn: the second scratch card's foreman.
pub const CLAUDE: Leg = Leg { key: "live-2/foreman", place: super::scratch::CLAUDE_CARD, to: "foreman", cli: "claude" };
/// Codex's turn: the scratch card's foreman.
pub const CODEX: Leg = Leg { key: "live-1/foreman", place: super::scratch::CARD, to: "foreman", cli: "codex" };

// Claude Code backgrounds a standalone `sleep`, so Claude's minute of work is F-C5's wait (3.1).
const CLAUDE_WORK: &str =
    "Run `python3 -c 'import time; time.sleep(60)'` in the foreground and wait for it to finish, then reply.";
const LONG_WORK: &str =
    "Run `python3 -c 'import time; time.sleep(120)'` in the foreground and wait for it to finish, then reply.";
pub(super) const TURN: Duration = Duration::from_secs(600);

/// Steps 1 to 4.
pub fn run(s: &mut Scratch, report: &mut Report) {
    let first = match start(s) {
        Ok(jobs) => jobs,
        Err(why) => return report.check("a walled turn of each CLI runs", Err(why)),
    };
    report.check("a walled turn of each CLI runs", Ok(()));
    if interrupted() {
        return;
    }
    s.kill();
    report.check("both carry on with no app", carry_on(s, &first));
    let again = s.start();
    let up = again.is_ok();
    report.check("the factory starts again", again);
    if !up || interrupted() {
        return;
    }
    report.check("both results applied once after the restart", applied_once(s, &first));
    report.check("no process of either job left", first.iter().try_for_each(|j| nothing_left(s, j)));
    if interrupted() {
        return;
    }
    report.check("a resumed turn of each", resumed(s));
    if interrupted() {
        return;
    }
    report.check("a turn stopped mid-tool leaves nothing", stopped(s));
}

pub(super) fn wait<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let start = factory_engine::clock::instant();
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if interrupted() {
            return Err(format!("interrupted waiting for {what}"));
        }
        if factory_engine::clock::elapsed(start) > limit {
            return Err(format!("no {what} in {}s", limit.as_secs()));
        }
        factory_engine::clock::sleep(Duration::from_millis(300));
    }
}

fn jobs_of(s: &Scratch, key: &str) -> Vec<Job> {
    let mut jobs: Vec<Job> =
        s.state().map(|st| st.jobs.into_values().filter(|j| j.owner.as_str() == key).collect()).unwrap_or_default();
    jobs.sort_by_key(|j| j.spec.created_ms);
    jobs
}

// The turn after the `before`th of an agent, once it is claimed.
fn next_turn(s: &Scratch, key: &str, before: usize) -> Result<Job, String> {
    wait(&format!("turn of {key}"), TURN, || {
        jobs_of(s, key).into_iter().nth(before).filter(|j| j.status != JobStatus::Launching)
    })
}

/// A turn of a leg, asked `text` in its place.
pub fn turn(s: &Scratch, leg: &Leg, text: &str) -> Result<Job, String> {
    let before = jobs_of(s, leg.key).len();
    s.ask(&json!({"say": {"place": leg.place, "to": leg.to, "text": text}}))?;
    next_turn(s, leg.key, before)
}

// Claude's card started, its foreman picked onto Opus once its first turn, on its seat's CLI, has made it: every
// later turn is Claude's (fac-119).
fn on_claude(s: &Scratch) -> Result<(), String> {
    s.ask(&json!({"start": {"card": CLAUDE.place, "mode": "report"}}))?;
    let made = |st: &State| st.agents.contains_key(&AgentKey::new(CLAUDE.key));
    wait(&format!("{} made", CLAUDE.key), TURN, || s.state().filter(made))?;
    let pick = json!({"agent": CLAUDE.key, "cli": "claude", "model": factory_types::config::OPUS, "effort": "high"});
    s.ask(&json!({"pick": pick}))?;
    let first = next_turn(s, CLAUDE.key, 0)?;
    let idle = |st: State| {
        let busy = st.jobs.values().any(|j| j.owner.as_str() == CLAUDE.key && j.settled.is_none());
        let owed = st.agents.get(&AgentKey::new(CLAUDE.key)).is_some_and(|a| a.owed.is_some());
        (!busy && !owed).then_some(())
    };
    wait(&format!("end of {}", first.id), TURN, || s.state().and_then(idle))
}

// Codex's first turn is its report card's, the card asking the work.
fn start(s: &Scratch) -> Result<Vec<Job>, String> {
    on_claude(s)?;
    let mut jobs = vec![turn(s, &CLAUDE, CLAUDE_WORK)?];
    s.ask(&json!({"start": {"card": super::scratch::CARD, "mode": "report"}}))?;
    jobs.push(next_turn(s, CODEX.key, 0)?);
    for (leg, job) in [&CLAUDE, &CODEX].into_iter().zip(&jobs) {
        if job.spec.wall != WallMode::Full {
            return Err(format!("{}'s turn isn't walled", leg.key));
        }
        if job.spec.argv.first().map(String::as_str) != Some(leg.cli) {
            return Err(format!("{}'s turn doesn't run {}: {:?}", leg.key, leg.cli, job.spec.argv.first()));
        }
    }
    // Into their work: the CLIs have started their commands, and both still run.
    factory_engine::clock::sleep(Duration::from_secs(5));
    for j in &jobs {
        if s.state().and_then(|st| st.jobs.get(&j.id).map(|x| x.status.clone())) != Some(JobStatus::Running) {
            return Err(format!("{} ended before the app was killed: a minute's work was asked", j.id));
        }
    }
    Ok(jobs)
}

fn dir(s: &Scratch, j: &Job) -> JobDir {
    JobDir::new(s.home.job(j.id.as_str()))
}

// With the app dead, each job's supervisor and wall run on.
fn carry_on(s: &Scratch, jobs: &[Job]) -> Result<(), String> {
    for j in jobs {
        let d = dir(s, j);
        let sup = d.claim().ok().flatten().map(|c| c.pid).ok_or(format!("{} has no claim", j.id))?;
        let finished = d.exit().ok().flatten().is_some();
        if !alive(sup) && !finished {
            return Err(format!("{}'s supervisor died with the app", j.id));
        }
    }
    Ok(())
}

// Both settle after the restart, each applied once.
fn applied_once(s: &Scratch, jobs: &[Job]) -> Result<(), String> {
    for j in jobs {
        let settled = wait(&format!("end of {}", j.id), TURN, || {
            s.state().and_then(|st| st.jobs.get(&j.id).and_then(|x| x.settled.clone()))
        })?;
        if settled != Settlement::Applied {
            return Err(format!("{} settled {settled:?}", j.id));
        }
        let n = s.events("job.settled").iter().filter(|e| e["job"] == j.id.as_str()).count();
        if n != 1 {
            return Err(format!("{} settled {n} times", j.id));
        }
    }
    Ok(())
}

// Nothing of a job runs: its supervisor and its wall are gone, and it left no process.
fn nothing_left(s: &Scratch, j: &Job) -> Result<(), String> {
    let d = dir(s, j);
    let exit = wait(&format!("exit.json of {}", j.id), Duration::from_secs(30), || d.exit().ok().flatten())?;
    if !exit.left.is_empty() {
        return Err(format!("{} left {:?}", j.id, exit.left));
    }
    let run = d.run().ok().flatten();
    let pids = [d.claim().ok().flatten().map(|c| c.pid), run.as_ref().map(|r| r.bwrap_pid), run.map(|r| r.init_pid)];
    match pids.into_iter().flatten().find(|p| *p > 0 && alive(*p)) {
        Some(p) => Err(format!("{}'s process {p} still runs", j.id)),
        None => Ok(()),
    }
}

fn resumed(s: &Scratch) -> Result<(), String> {
    for leg in [&CLAUDE, &CODEX] {
        let j = turn(s, leg, "Reply in one line: what did you run last?")?;
        if !j.spec.argv.iter().any(|a| a == "--resume" || a == "resume") {
            return Err(format!("{}'s second turn didn't resume its session: {:?}", leg.key, j.spec.argv));
        }
        applied_once(s, &[j])?;
    }
    Ok(())
}

// A line cut by the read's timeout is kept until its end comes.
struct Watch(std::io::BufReader<std::os::unix::net::UnixStream>, String);

impl Watch {
    fn new(s: &Scratch, place: &str) -> Result<Self, String> {
        use std::io::Write;
        let mut stream =
            std::os::unix::net::UnixStream::connect(s.home.factory_sock()).map_err(|e| format!("watch: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(1))).map_err(|e| e.to_string())?;
        let watch = json!({"id": 1, "cmd": {"watch": {"places": [place], "full": true}}});
        stream.write_all(format!("{watch}\n").as_bytes()).map_err(|e| format!("watch: {e}"))?;
        Ok(Self(std::io::BufReader::new(stream), String::new()))
    }

    fn steps(&mut self, agent: &str) -> Result<Option<Vec<serde_json::Value>>, String> {
        use std::io::BufRead;
        match self.0.read_line(&mut self.1) {
            Ok(0) => return Err("the factory closed the watch".into()),
            Ok(_) if self.1.ends_with('\n') => {}
            Ok(_) | Err(_) => return Ok(None),
        }
        let line = std::mem::take(&mut self.1);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        let Some(rows) = v.pointer("/event/view/agents").and_then(serde_json::Value::as_array) else {
            return Ok(None);
        };
        let row = rows.iter().find(|a| a["agent"] == agent);
        Ok(Some(row.and_then(|a| a["steps"].as_array().cloned()).unwrap_or_default()))
    }
}

// Waits until the turn's steps, as the engine read them from its stream, show its tool running (M5.21):
// steps, and not the ones shown before the turn. A turn that ends first fails.
fn turn_in_tool(s: &Scratch, leg: &Leg, text: &str) -> Result<Job, String> {
    let agent = leg.key;
    let mut watch = Watch::new(s, leg.place)?;
    let start = factory_engine::clock::instant();
    let before = loop {
        if let Some(steps) = watch.steps(agent)? {
            break steps;
        }
        if factory_engine::clock::elapsed(start) > Duration::from_secs(30) || interrupted() {
            return Err(format!("{}'s view never came", leg.place));
        }
    };
    let j = turn(s, leg, text)?;
    while factory_engine::clock::elapsed(start) < TURN && !interrupted() {
        if watch.steps(agent)?.is_some_and(|steps| !steps.is_empty() && steps != before) {
            return Ok(j);
        }
        if s.state().and_then(|st| st.jobs.get(&j.id).and_then(|x| x.settled.clone())).is_some() {
            return Err(format!("{} ended before its steps showed its tool running", j.id));
        }
    }
    Err(format!("{}'s steps never showed its tool running in {}s", j.id, TURN.as_secs()))
}

fn stopped(s: &Scratch) -> Result<(), String> {
    let j = turn_in_tool(s, &CLAUDE, LONG_WORK)?;
    s.ask(&json!({"stop": {"agent": CLAUDE.key}}))?;
    let settled =
        wait("the stop", TURN, || s.state().and_then(|st| st.jobs.get(&j.id).and_then(|x| x.settled.clone())))?;
    if settled != Settlement::Stopped {
        return Err(format!("it settled {settled:?}"));
    }
    nothing_left(s, &j)
}
