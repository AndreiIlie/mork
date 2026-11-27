// Watching jobs (1.4.6, 1.4.7): their output read as it grows, one complete line at a time;
// their end noticed when the lock is let go; their result applied once.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use std::time::Duration;

use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::config::Cli;
use mork_types::job::{JobKind, WallMode};
use mork_types::seen::{StepList, TurnView};
use mork_types::state::JobStatus;
use mork_types::{AgentKey, JobId};

use super::{Engine, Live};
use crate::jobs::folder::JobDir;
use crate::jobs::stamps::Stamps;
use crate::jobs::{procs, published};
use crate::{builds, cli, clock};

const CLAIM_WAIT: Duration = Duration::from_secs(5);
// What is read of a retained stream's end, to show its turn again after a restart.
const RESTORE_MAX: u64 = 4 * 1024 * 1024;

/// The CLI an agent's job runs, read off its own command: its seat, or a pick, may say another
/// since it launched (fac-96).
pub(super) fn cli_of(state: &mork_core::state::State, id: &JobId) -> Option<Cli> {
    let j = state.jobs.get(id).filter(|j| state.agents.contains_key(&j.owner))?;
    j.spec.argv.first().and_then(|w| Cli::parse(w))
}

impl Engine {
    pub(super) fn watch_jobs(&mut self) {
        let ids: Vec<JobId> = self.state.jobs.values().filter(|j| j.settled.is_none()).map(|j| j.id.clone()).collect();
        for id in ids {
            self.watch_one(&id);
        }
    }

    fn live_of(&mut self, id: &JobId) -> &mut Live {
        let cli = cli_of(&self.state, id);
        let stream = self.stream_for(id);
        // Reattached: its limits were applied up to here, with the state (M2.2).
        let replayed_to = self.state.jobs.get(id).map_or(0, |j| j.limits_at);
        let dir = self.job_dir(id);
        self.live.entry(id.clone()).or_insert_with(|| {
            let held = std::fs::metadata(dir.out()).map_or(0, |m| m.len());
            Live {
                offset: 0,
                stream,
                spawned: clock::instant(),
                cli,
                reported: Vec::new(),
                replayed_to,
                stamps: Stamps::watching(dir.stamps(), held),
                ended: false,
            }
        })
    }

    pub(super) fn refresh_recipient(&mut self, place: &str, seq: u64) {
        if let Some(id) = mork_core::messages::receipts::running_recipient(&self.state, place, seq) {
            self.watch_one(&id);
        }
    }

    fn watch_one(&mut self, id: &JobId) {
        let dir = self.job_dir(id);
        let launching = self.state.jobs.get(id).is_some_and(|j| j.status == JobStatus::Launching);
        let claimed = dir.claim_path().exists();
        if launching && claimed {
            self.apply(&Event::JobRunning { id: id.clone() });
        }
        self.read_lines(id, &dir, false);
        if !dir.exists() {
            return;
        }
        if dir.lock_held() {
            self.end_reported(id, &dir);
            return;
        }
        if claimed {
            self.settle(id);
        } else if clock::elapsed(self.live_of(id).spawned) > CLAIM_WAIT {
            if self.state.jobs.get(id).is_some_and(|j| j.stop_requested) {
                self.apply(&Event::JobEnded { id: id.clone(), end: JobEnd::Stopped });
            } else {
                self.start_supervisor(id);
            }
        }
    }

    // A Claude turn's first usable report is its reply: a background task's later notification would only
    // ask for another (fac-321). Its supervisor sees what is asked to end go, and records its exit as ever.
    fn end_reported(&mut self, id: &JobId, dir: &JobDir) {
        let stopping = self.state.jobs.get(id).is_none_or(|j| j.stop_requested);
        let wall = self.state.jobs.get(id).map(|j| j.spec.wall);
        let Some(live) = self.live.get_mut(id) else { return };
        let reported = matches!(&live.stream.result, Some(Ok(v)) if !v.is_string());
        if stopping || live.ended || live.cli != Some(Cli::Claude) || !reported {
            return;
        }
        let Ok(Some(run)) = dir.run() else { return };
        live.ended = true;
        if wall == Some(WallMode::None) {
            procs::signal_all(&procs::unwalled(run.pgid, id.as_str(), None), procs::libc_sig::TERM);
        } else if procs::start_time(run.init_pid) == Some(run.init_start) {
            procs::term_walled(&run.pidns, run.init_pid);
        }
    }

    // Only complete lines are read: at the end, a last line without its newline is logged and ignored (1.4.7.4).
    // Only an agent's turn has a stream to read: other jobs settle from their files.
    fn read_lines(&mut self, id: &JobId, dir: &JobDir, at_end: bool) {
        let Live { offset, cli: Some(_), .. } = *self.live_of(id) else { return };
        let Ok(mut f) = std::fs::File::open(dir.out()) else { return };
        if f.seek(SeekFrom::Start(offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return;
        }
        let complete = buf.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
        let now = clock::now_ms();
        let app_server = self.state.jobs.get(id).is_some_and(|j| cli::app_server(&j.spec));
        let live = self.live_of(id);
        let mut end = live.offset;
        for raw in buf[..complete].split_inclusive(|&b| b == b'\n') {
            end += raw.len() as u64;
            // A line read before, as after a restart, keeps the time it was first read.
            live.stream.at = if live.stamps.covers(end) { live.stamps.at(end) } else { Some(now) };
            let line = String::from_utf8_lossy(raw);
            if let Some(c) = live.cli {
                cli::read_line(c, app_server, &mut live.stream, line.trim_end_matches('\n'));
            }
            // What lies at or before the applied position was applied before (1.4.7.2).
            if end <= live.replayed_to {
                live.reported = live.stream.limits.clone();
            }
        }
        live.offset += complete as u64;
        live.stamps.read_to(live.offset, now);
        if at_end && complete < buf.len() {
            let tail: String = String::from_utf8_lossy(&buf[complete..]).chars().take(200).collect();
            self.log(&format!("{id}: a last line with no newline, ignored: {tail}"));
        }
        if complete > 0 {
            self.stream_seen(id);
        }
    }

    fn stream_seen(&mut self, id: &JobId) {
        let Some(owner) = self.state.jobs.get(id).map(|j| j.owner.clone()) else { return };
        let Some(live) = self.live.get_mut(id) else { return };
        let seen = super::AgentSeen {
            steps: live.stream.steps.iter().map(|s| s.words.clone()).collect(),
            doing: live.stream.doing.clone(),
            context: live.stream.context.clone(),
            turn: Some(Arc::new(cli::turn::turn_of(&live.stream, id))),
            list: Some(StepList { job: id.clone(), steps: live.stream.list.clone() }),
        };
        // Only the windows read past what was applied: older ones may be staler than the state.
        let windows: Vec<_> = live.stream.limits.iter().filter(|w| !live.reported.contains(w)).cloned().collect();
        let fresh = !windows.is_empty();
        live.reported = live.stream.limits.clone();
        let at = (id.clone(), live.offset);
        let cli = live.cli;
        let moved = self.seen.get(&owner).is_none_or(|old| {
            (&old.steps, &old.doing, &old.context, &old.turn, &old.list)
                != (&seen.steps, &seen.doing, &seen.context, &seen.turn, &seen.list)
        });
        let read: Vec<_> = seen.turn.iter().flat_map(|t| t.read.iter().map(|r| r.seq)).collect();
        if read.is_empty() {
            self.state.world.live_reads.remove(id);
        } else {
            self.state.world.live_reads.insert(id.clone(), read);
        }
        self.seen.insert(owner, seen);
        // A step shows on the next of the tick's publishes, a few a second at most (1.5).
        if moved {
            self.views_cache.stale = true;
        }
        if let Some(cli) = cli.filter(|_| fresh) {
            let limits = super::limits_of(cli, &windows, clock::now_ms());
            self.apply(&Event::LimitsSeen { limits, at: Some(at), whole: None });
        }
    }

    /// After a restart, each watched agent's latest turn, when it ended before, read again from
    /// its retained stream by its own CLI's parser: with none when its logs were pruned (fac-96).
    pub(super) fn restore_turns(&mut self) {
        let places: BTreeSet<&str> =
            self.watchers.iter().flat_map(|(_, w)| w.places.iter().map(String::as_str)).collect();
        let due: Vec<(AgentKey, Option<JobId>)> = self
            .state
            .agents
            .values()
            .filter(|a| places.contains(a.place.as_str()) && !self.seen.contains_key(&a.key))
            .map(|a| {
                let j = self.state.jobs.values().filter(|j| j.owner == a.key).max_by_key(|j| j.spec.created_ms);
                let ended = j.filter(|j| j.kind == JobKind::Turn && j.settled.is_some());
                (a.key.clone(), ended.map(|j| j.id.clone()))
            })
            .collect();
        // An agent with no ended turn is looked at once too: its next turn's start resets it.
        for (agent, id) in due {
            let turn = id.and_then(|id| self.retained_turn(&id)).map(Arc::new);
            self.views_cache.stale |= turn.is_some();
            self.seen.insert(agent, super::AgentSeen { turn, ..Default::default() });
        }
    }

    pub(super) fn retained_turn(&self, id: &JobId) -> Option<TurnView> {
        let job = self.state.jobs.get(id).filter(|j| j.kind == JobKind::Turn)?;
        let cli = job.spec.argv.first().and_then(|w| Cli::parse(w))?;
        let mut f = std::fs::File::open(self.job_dir(id).out()).ok()?;
        let from = f.metadata().ok()?.len().saturating_sub(RESTORE_MAX);
        f.seek(SeekFrom::Start(from)).ok()?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        let mut st = self.stream_for(id);
        let stamps = Stamps::load(self.job_dir(id).stamps());
        let mut end = from;
        // A start cut mid-line is a line that reads as none.
        for raw in buf.split_inclusive(|&b| b == b'\n').filter(|l| l.ends_with(b"\n")) {
            end += raw.len() as u64;
            st.at = stamps.at(end);
            let line = String::from_utf8_lossy(raw);
            cli::read_line(cli, cli::app_server(&job.spec), &mut st, line.trim_end());
        }
        // Read only once its job ended: what it left running ended with it.
        cli::turn::close(&mut st, stamps.last().max(job.settled_ms));
        let mut turn = cli::turn::turn_of(&st, id);
        turn.cut |= from > 0;
        Some(turn)
    }

    /// The job ended: its result, if it has a complete one, applied once (2.4).
    pub(super) fn settle(&mut self, id: &JobId) {
        let Some(job) = self.state.jobs.get(id).cloned() else { return };
        let dir = self.job_dir(id);
        let exit = dir.exit().ok().flatten();
        if exit.is_none() {
            self.end_leftovers(&dir, &job.spec.wall, id);
        }
        if job.kind == JobKind::Build {
            self.settle_build(&job, exit.as_ref());
            return;
        }
        if job.purpose.is_some() {
            self.settle_work(&job, exit.as_ref());
            return;
        }
        let (stream, session) = self.read_session(&job, &dir);
        // A stop or a failure carries no session: what the stream showed is kept first (1.4.8, fac-629).
        if session.started || session.thread.is_some() || session.compacted || session.tool_counts.is_some() {
            self.apply(&Event::SessionSeen { id: id.clone(), session: session.clone() });
        }
        let exit_facts = exit.as_ref().map(|e| (e.stopped, e.error.clone()));
        let mut end =
            mork_core::agents::recovery::end_of(job.stop_requested, exit_facts, stream.result.clone(), session, || {
                self.cli_words(&dir)
            });
        // The reply's images become copies read beneath this job's `$MORK_IO`, before anything reads them (fac-278).
        if let JobEnd::Finished { reply: Ok(reply), .. } = &mut end {
            published::publish(&self.opts.home.jobs(), id.as_str(), &dir.mork_io(&job.spec), reply);
        }
        self.apply(&Event::JobEnded { id: id.clone(), end });
        self.close_turn(id);
        self.live.remove(id);
    }

    fn read_session(&mut self, job: &mork_types::state::Job, dir: &JobDir) -> (cli::StreamState, SessionSeen) {
        self.read_lines(&job.id, dir, true);
        let mut stream = self.live.get(&job.id).map(|l| l.stream.clone()).unwrap_or_default();
        if self.live.get(&job.id).and_then(|l| l.cli) == Some(Cli::Codex) {
            let last = std::fs::read_to_string(dir.io().join("last-message.json")).ok();
            cli::codex::finish(&mut stream, last.as_deref());
        }
        if self.live.get(&job.id).and_then(|l| l.cli) == Some(Cli::Agy) {
            let stderr = std::fs::read_to_string(dir.err()).unwrap_or_default();
            cli::agy::settle(&job.spec.argv, &mut stream, &stderr);
        }
        let session = SessionSeen {
            started: stream.session_started,
            thread: stream.thread.clone(),
            limit_reached: stream.limit_rejected.is_some(),
            limit_reset: stream.limit_rejected.flatten(),
            cost_micros: stream.cost_usd.map(cli::micros),
            selection: mork_types::cost::Selection::from_argv(&job.spec.argv),
            token_totals: stream.token_totals.clone(),
            window: cli::window_of(&stream),
            context: stream.context.as_ref().map(|c| c.used),
            tools: !stream.steps.is_empty(),
            tool_counts: self.live.get(&job.id).filter(|live| live.offset > 0).map(|_| stream.tool_counts()),
            taken: stream.taken.iter().filter_map(|u| cli::live::seq_of(job.id.as_str(), u)).collect(),
            answered: stream.answered || stream.result.as_ref().is_some_and(Result::is_ok),
            compacted: stream.compacted,
        };
        (stream, session)
    }

    /// Recovery's "any other combination": what is left of the job is ended, and it is
    /// recorded cut, whatever its folder holds (1.4.6.1).
    pub(super) fn settle_cut(&mut self, id: &JobId) {
        let Some(job) = self.state.jobs.get(id).cloned() else { return };
        let dir = self.job_dir(id);
        self.end_leftovers(&dir, &job.spec.wall, id);
        if job.kind == JobKind::Build {
            let commit = builds::commit_of(&job.spec);
            self.apply(&Event::BuildEnded { id: id.clone(), commit, result: Err("cut".into()), pinned: None });
        } else {
            let session = if job.kind == JobKind::Turn && job.purpose.is_none() {
                self.read_session(&job, &dir).1
            } else {
                SessionSeen::default()
            };
            self.apply(&Event::JobEnded { id: id.clone(), end: JobEnd::Cut { session } });
            self.close_turn(id);
        }
        self.live.remove(id);
    }

    // The agent's turn shows the calls its ended job left running ended with it, when it settled,
    // as its stream read again shows them (fac-206).
    fn close_turn(&mut self, id: &JobId) {
        let settled = self.state.jobs.get(id).and_then(|j| j.settled_ms);
        let Some(live) = self.live.get_mut(id) else { return };
        let last = live.stamps.last().max(settled);
        cli::turn::close(&mut live.stream, last);
        self.stream_seen(id);
    }

    fn settle_build(&mut self, job: &mork_types::state::Job, exit: Option<&mork_types::job::ExitRecord>) {
        let ok = exit.is_some_and(|e| e.code == Some(0));
        let err = std::fs::read_to_string(self.job_dir(&job.id).err()).unwrap_or_default();
        // The state keeps the build's error as the operator reads it.
        let result = builds::finish(&self.opts.home, &job.spec, ok, &err).map_err(|e| e.to_string());
        let commit = builds::commit_of(&job.spec);
        // Pinned before the end is applied: a crash between them pins it again, or finds it pinned (fac-19).
        let pinned = if result.is_ok() { self.update_pin(&commit) } else { None };
        let ended = Event::BuildEnded { id: job.id.clone(), commit, result, pinned };
        self.apply(&ended);
        self.live.remove(&job.id);
    }

    fn cli_words(&self, dir: &JobDir) -> String {
        let err = std::fs::read_to_string(dir.err()).unwrap_or_default();
        let out = std::fs::read_to_string(dir.out()).unwrap_or_default();
        let tail: Vec<&str> = out.lines().rev().take(3).collect();
        let mut w = err.trim().to_owned();
        for l in tail.into_iter().rev() {
            if !w.is_empty() {
                w.push('\n');
            }
            w.push_str(l);
        }
        if w.is_empty() { "the CLI ended with no result and no words".into() } else { w }
    }

    fn end_leftovers(&self, dir: &JobDir, wall: &WallMode, id: &JobId) {
        let Ok(Some(run)) = dir.run() else { return };
        if *wall == WallMode::None {
            let _ = procs::end_unwalled(run.pgid, id.as_str(), None);
        } else if procs::start_time(run.init_pid) == Some(run.init_start) {
            let _ = procs::end_walled(&run.pidns, run.init_pid);
        }
    }

    /// Stopping (1.4.5): the stop is in the state already; signal the supervisor, or, with
    /// none holding the lock, end the job's processes here.
    pub(super) fn stop_job(&mut self, id: &JobId) {
        let dir = self.job_dir(id);
        if dir.lock_held()
            && let Ok(Some(claim)) = dir.claim()
            && procs::start_time(claim.pid) == Some(claim.start_time)
        {
            let _ = mork_sys::kill(claim.pid, procs::libc_sig::TERM);
            return;
        }
        // Not claimed yet: claim it here, so no supervisor ever starts it, and settle it.
        if dir.exists() && !dir.claim_path().exists() {
            let made = std::fs::OpenOptions::new().write(true).create_new(true).open(dir.claim_path());
            if made.is_ok() {
                self.apply(&Event::JobEnded { id: id.clone(), end: JobEnd::Stopped });
                return;
            }
        }
        if !dir.exists() {
            self.apply(&Event::JobEnded { id: id.clone(), end: JobEnd::Stopped });
            return;
        }
        let wall = self.state.jobs.get(id).map(|j| j.spec.wall).unwrap_or(WallMode::Full);
        self.end_leftovers(&dir, &wall, id);
    }
}
