// Commands (1.5, 1.6): parsed strictly, their sender known by the socket, answered by id.
// Most go to `core`; reads, builds, pins and reloads are the engine's.

use std::sync::Arc;
use std::time::Duration;

use mork_core::decide::Event;
use mork_types::Command;
use mork_types::JobId;
use mork_types::config::Config;
use mork_types::protocol::{ErrorBody, ErrorCode, Request};
use mork_types::state::{ORCHESTRATOR, Sender};
use serde_json::{Value, json};

use super::{Engine, Input, Phase};
use crate::jobs::folder::JobDir;
use crate::sockets::{Conn, Incoming};
use crate::{builds, clock};

fn err(code: ErrorCode, m: impl Into<String>) -> Result<Value, ErrorBody> {
    Err(ErrorBody::new(code, m))
}

// Work jobs need purpose attribution; agent turns carry their card only in their owner key (fac-280).
fn job_card(j: &mork_types::state::Job) -> Option<&str> {
    j.purpose.as_ref().and_then(|p| p.card()).map(|c| c.as_str()).or_else(|| j.owner.as_str().split('/').next())
}

// Who a request counts as: its socket's sender, or a configured project's orchestrator on the operator's
// socket (fac-119, fac-244).
fn sent_as(sent_as: Option<&str>, from: Option<Sender>, config: &Config) -> Result<Sender, ErrorBody> {
    let Some(from) = from else {
        return Err(ErrorBody::new(ErrorCode::Refused, "this connection comes from no running job"));
    };
    let Some(name) = sent_as else { return Ok(from) };
    let Some(project) = name.strip_prefix(ORCHESTRATOR) else {
        let gone = if name == "pa" { ": the projects' orchestrators replaced the PA (fac-244)" } else { "" };
        let words = format!("`as` is `{ORCHESTRATOR}<project>` or nothing, not `{name}`{gone}");
        return Err(ErrorBody::new(ErrorCode::Invalid, words));
    };
    if from != Sender::Operator {
        let words = "`--as` is for the operator's socket: an agent always sends as itself";
        return Err(ErrorBody::new(ErrorCode::Refused, words));
    }
    if !config.projects.contains_key(project) {
        let words = format!("no project `{project}`: `--as {ORCHESTRATOR}<project>` names a configured project");
        return Err(ErrorBody::new(ErrorCode::Invalid, words));
    }
    Ok(Sender::Orchestrator(project.into()))
}

impl Engine {
    pub(super) fn incoming(&mut self, inc: Incoming) {
        match inc {
            Incoming::Request { conn, req, sender, job } => {
                let id = req.id;
                if let Some(result) = self.request(&conn, req, sender, job) {
                    conn.answer(id, result);
                }
            }
            Incoming::Closed { conn } => {
                self.watchers.retain(|(c, _)| c.id != conn);
                self.hook_closed(conn);
                self.waiting_wakes_closed(conn);
                self.conn_closed(conn);
                self.door_closed(conn);
            }
        }
    }

    fn request(
        &mut self,
        conn: &Arc<Conn>,
        req: Request,
        sender: Option<Sender>,
        job: Option<JobId>,
    ) -> Option<Result<Value, ErrorBody>> {
        let cmd = match Command::parse(&req.cmd, self.faults()) {
            Ok(c) => c,
            Err(inv) => return Some(err(ErrorCode::Invalid, inv.message)),
        };
        let from = match sent_as(req.sent_as.as_deref(), sender, &self.ctx.config) {
            Ok(from) => from,
            Err(e) => return Some(Err(e)),
        };
        // What the command reads of an ended card is taken in before any handler decides (fac-336).
        if !matches!(cmd, Command::HistoryJournal { .. }) {
            self.load_wanted(&Event::Command { from: from.clone(), cmd: cmd.clone() });
        }
        // `buzz` runs on every command before any handler, the engine's own included (3.6.3,
        // M3.20): a buzzed one goes to `core`, which refuses it and journals the buzz.
        if mork_core::decide::buzz::buzz(&self.state, &from, &cmd, self.ctx_now()).is_some() {
            let d = self.apply(&Event::Command { from, cmd });
            return Some(d.answer.unwrap_or_else(|| err(ErrorCode::Buzz, "buzzed")));
        }
        if let Err(e) = self.review_reply_words(&cmd) {
            return Some(Err(e));
        }
        let reload = matches!(cmd, Command::Reload { .. });
        if matches!(self.phase, Phase::Reloading { .. }) && cmd.acts_outward() && !reload {
            return Some(err(ErrorCode::Reloading, "a reload is under way"));
        }
        Some(match cmd {
            Command::Watch { places, tail, full } => {
                return self.watch(conn, req.id, places, tail, full.unwrap_or(false));
            }
            Command::Hook {} => match &from {
                Sender::Orchestrator(project) => return self.hook(conn, req.id, &project.clone()),
                _ => err(ErrorCode::Refused, "`hook` is a project's orchestrator's"),
            },
            Command::Wakes {} => match &from {
                Sender::Orchestrator(project) => return self.wait_wakes(conn, req.id, project),
                _ => err(ErrorCode::Refused, "`wakes` is a project's orchestrator's"),
            },
            Command::History { place, before, count, turns } => Ok(self.history(&place, before, count, turns)),
            Command::HistorySearch { words } => return self.history_search(conn, req.id, &from, words),
            cmd @ Command::HistoryJournal { .. } => return self.history_journal(conn, req.id, &from, cmd),
            Command::Status { jobs, all, card } => Ok(self.status(jobs, all, card.as_deref(), conn.origin)),
            Command::Quality {} => self.quality(),
            Command::Usage {} => Ok(self.usage()),
            Command::ClaudeAccount { name } => self.claude_account(&name),
            Command::DebugSleep { ms } => return self.debug_sleep(conn, req.id, ms),
            Command::Build { commit } => self.start_build(&commit, None),
            Command::Pin { commit } => self.pin_cmd(&commit, &from),
            Command::Reload { target } => self.reload_cmd(target.as_deref(), &from),
            cmd @ (Command::CardCreate { .. } | Command::CardChange { .. } | Command::CardClose { .. }) => {
                self.board_request(cmd, from)
            }
            cmd @ (Command::TestLease { .. } | Command::TestResume { .. } | Command::TestDone { .. }) => {
                return self.slot_request(conn, req.id, cmd, from);
            }
            Command::Door { target, op, params } => {
                return self.door_request(conn, req.id, super::door::Asked { target, op, params, from, job });
            }
            Command::DoorHelp { target } => self.door_help(target.as_deref()),
            Command::Preview { card, action } => self.preview_cmd(card, action, false, from),
            Command::Demo { card } => self.preview_cmd(card, None, true, from),
            other => self.decided(other, from, job.as_ref()),
        })
    }

    fn quality(&self) -> Result<Value, ErrorBody> {
        let quality =
            self.store.quality(clock::now_ms()).map_err(|e| ErrorBody::new(ErrorCode::Refused, e.to_string()))?;
        Ok(json!(mork_types::quality::QualityAnswer { quality }))
    }

    fn claude_account(&mut self, name: &str) -> Result<Value, ErrorBody> {
        let refused = |message| ErrorBody::new(ErrorCode::Refused, message);
        let home = self.opts.home.login_home();
        let switch = crate::claude_accounts::Switch::prepare(
            &home,
            &self.opts.home.logins(),
            name,
            &self.ctx.config.claude_accounts,
        )
        .map_err(refused)?;
        self.forget_claude_readings().map_err(refused)?;
        let result = switch.apply();
        self.views_cache.claude_accounts.reload(&home);
        self.align_claude_readings();
        self.publish_screen();
        result.map_err(refused)?;
        Ok(json!({"claude.account": {"account": switch.account, "replaced": switch.replaced}}))
    }

    fn review_reply_words(&self, cmd: &Command) -> Result<(), ErrorBody> {
        let text = match cmd {
            Command::Answer { text, .. } | Command::Resolve { dropped: Some(text), .. } => text,
            _ => return Ok(()),
        };
        let secrets = crate::secrets::Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        super::act_forge::check_words(&self.opts.home, &self.ctx.config, &secrets, text, "the reply on the PR").map_err(
            |failure| {
                let (code, words) = match failure {
                    mork_types::work::ActionFailure::Leak { words } => (ErrorCode::Leak, words),
                    mork_types::work::ActionFailure::Internals { words } => (ErrorCode::Internals, words),
                    other => (ErrorCode::Refused, format!("{other:?}")),
                };
                ErrorBody::new(code, format!("Reword the reply: {words}; nothing was recorded."))
            },
        )
    }

    fn forget_claude_readings(&mut self) -> Result<(), String> {
        for job in self.state.jobs.values().filter(|job| {
            job.settled.is_none()
                && job.kind == mork_types::job::JobKind::Turn
                && job.spec.argv.first().map(String::as_str) == Some("claude")
        }) {
            JobDir::new(self.opts.home.job(job.id.as_str()))
                .forget_usage_account()
                .map_err(|e| format!("could not discard {}'s Claude reading account: {e}", job.id))?;
            if let Some(live) = self.live.get_mut(&job.id) {
                live.stream.account = None;
                live.stream.limits.clear();
            }
        }
        Ok(())
    }

    pub(super) fn align_claude_readings(&mut self) {
        let active = self.views_cache.claude_accounts.live.as_ref().map(|a| &a.id);
        for live in self.live.values_mut().filter(|live| live.cli == Some(mork_types::config::Cli::Claude)) {
            if live.stream.account.as_ref().map(|a| &a.id) != active {
                live.stream.account = None;
                live.stream.limits.clear();
            }
        }
    }

    fn debug_sleep(&self, conn: &Arc<Conn>, id: u64, ms: u64) -> Option<Result<Value, ErrorBody>> {
        let tx = self.tx.clone();
        let conn = Arc::clone(conn);
        std::thread::spawn(move || {
            clock::sleep(Duration::from_millis(ms));
            let _ = tx.send(Input::Answer { conn, id, ok: json!({"debug.sleep": {"ms": ms}}) });
        });
        None
    }

    // A connection's watch replaces what it watched before: the window moves.
    fn watch(
        &mut self,
        conn: &Arc<Conn>,
        id: u64,
        places: Vec<String>,
        tail: Option<u64>,
        full: bool,
    ) -> Option<Result<Value, ErrorBody>> {
        // A former member may have closed with work of its own while its board card still looks runnable.
        let cards = places
            .iter()
            .filter_map(|place| mork_core::messages::places::card_of(&self.state, place))
            .filter_map(|id| self.state.cards.get(&id))
            .filter(|c| c.stage == mork_types::card::Stage::Closed)
            .flat_map(|c| c.carries.iter().map(|k| &k.id))
            .filter(|id| self.state.unloaded.cards.contains(*id))
            .cloned()
            .collect();
        self.load(&mork_core::state::unloaded::Wanted { cards, ..Default::default() });
        let watched = super::Watched::new(places.clone(), tail, full);
        self.watchers.retain(|(c, _)| c.id != conn.id);
        self.watchers.push((Arc::clone(conn), watched.clone()));
        conn.answer(id, Ok(json!({"watch": {"places": places}})));
        self.send_views(conn, &watched);
        None
    }

    fn status(
        &self,
        jobs: Option<bool>,
        all: Option<bool>,
        card: Option<&str>,
        origin: crate::sockets::Origin,
    ) -> Value {
        let lines = mork_core::views::status::status_lines(&self.state);
        let mut status = json!({"lines": lines, "revision": self.state.revision});
        if origin == crate::sockets::Origin::Operator {
            status["web"] = match self.dashboard_port() {
                Ok(port) => json!({"port": port}),
                Err(why) => json!({"unavailable": why}),
            };
        }
        if jobs.unwrap_or(false) {
            status["jobs"] = json!(self.status_jobs(all.unwrap_or(false), card));
        }
        json!({"status": status})
    }

    // The live jobs; `all` adds the settled ones whose folders are still kept, which the doctor and
    // build wait need (fac-280).
    fn status_jobs(&self, all: bool, card: Option<&str>) -> Vec<Value> {
        let folders: std::collections::BTreeSet<String> = if all {
            std::fs::read_dir(self.opts.home.jobs())
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        } else {
            Default::default()
        };
        let mut out: Vec<Value> = self
            .state
            .jobs
            .values()
            .filter(|j| j.settled.is_none() || folders.contains(j.id.as_str()))
            .filter(|j| card.is_none_or(|c| job_card(j) == Some(c)))
            .map(|j| json!({"id": j.id.as_str(), "kind": j.kind.as_str(), "settled": j.settled.is_some(), "settled_ms": j.settled_ms}))
            .collect();
        // A kept folder's job the state doesn't hold, an ended card's (fac-336).
        let stored =
            folders.iter().filter(|f| card.is_none() && !self.state.jobs.contains_key(&JobId::new(f.as_str())));
        for id in stored {
            if let Ok(Some((kind, settled_ms))) = self.store.job_seen(id) {
                out.push(json!({"id": id, "kind": kind, "settled": settled_ms.is_some(), "settled_ms": settled_ms}));
            }
        }
        out
    }

    /// A build: asked for, or owed by a landing (`purpose`, kept on its job).
    pub(super) fn start_build(
        &mut self,
        commit: &str,
        purpose: Option<&mork_types::work::Purpose>,
    ) -> Result<Value, ErrorBody> {
        let spec =
            builds::prepare(&self.ctx, commit, clock::now_ms()).map_err(|e| ErrorBody::new(ErrorCode::Refused, e))?;
        let id = spec.id.clone();
        let mut job = super::pump::job_of(spec, String::new(), Default::default());
        job.purpose = purpose.cloned();
        self.apply(&Event::JobRecorded { job: Box::new(job) });
        let Some(recorded) = self.state.jobs.get(&id) else {
            return Err(ErrorBody::new(ErrorCode::Refused, "the build couldn't be recorded"));
        };
        let commit = builds::commit_of(&recorded.spec);
        self.launch(&id);
        Ok(json!({"build": {"job": id.as_str(), "commit": commit}}))
    }

    // A pin is checked as a reload is (1.8.6.1).
    pub(super) fn refusal(&self, commit: &str) -> Result<String, builds::BuildError> {
        let unsettled = builds::unsettled(&self.state);
        let under_way = matches!(self.phase, Phase::Reloading { .. });
        builds::check_target(&self.opts.home, &self.ctx.config, commit, self.store.version, &unsettled, under_way)
    }

    fn pin_cmd(&mut self, commit: &str, from: &Sender) -> Result<Value, ErrorBody> {
        let full = self.refusal(commit).map_err(|e| ErrorBody::new(ErrorCode::Refused, e.to_string()))?;
        builds::write_pin(&self.opts.home, &full, &from.label())
            .map_err(|e| ErrorBody::new(ErrorCode::Refused, e.to_string()))?;
        Ok(json!({"pin": {"commit": full}}))
    }

    fn reload_cmd(&mut self, target: Option<&str>, from: &Sender) -> Result<Value, ErrorBody> {
        if matches!(self.phase, Phase::Reloading { .. }) {
            return err(ErrorCode::Refused, "another reload is under way");
        }
        let target = match target.map(str::to_owned).or_else(|| builds::pin(&self.opts.home)) {
            Some(t) => t,
            None => return err(ErrorCode::Refused, "nothing is pinned, and no target was given"),
        };
        let full = match self.refusal(&target) {
            Ok(f) => f,
            Err(e) => return err(ErrorCode::Refused, e.to_string()),
        };
        // The target moves the shortcut as it starts: refused here, while this app can carry on.
        if let Err(e) = builds::check_shortcut() {
            return err(ErrorCode::Refused, e.to_string());
        }
        if let Err(e) = builds::check_target_config(&self.opts.home, &full) {
            return err(ErrorCode::Refused, e.to_string());
        }
        let id = crate::random::uuid();
        // A tenth of a second at least before the exec, for the watchers' writers to send this
        // answer and the views that show the reload (5.1.3, M4.14).
        self.phase = Phase::Reloading {
            target: full.clone(),
            id: id.clone(),
            at: clock::instant() + Duration::from_millis(100),
            who: from.label(),
        };
        self.publish();
        Ok(json!({"reload": {"target": full, "id": id}}))
    }

    pub(super) fn job_dir(&self, id: &JobId) -> JobDir {
        JobDir::new(self.opts.home.job(id.as_str()))
    }
}
