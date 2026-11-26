// Codex's usage limits (1.4.8, Codex 7) and agy's while its token exists (fac-657): each CLI's read walled with the
// login home, one at a time, cut at 20 s, on a thread of its own so the engine never waits on it.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use mork_core::decide::Event;
use mork_types::AgentKey;
use mork_types::config::Cli;
use mork_types::job::WallMode;

use super::{Engine, Phase};
use crate::jobs::launch::{self, Answer};
use crate::{cli, clock, random};

/// The exchange, as F-X5 records it.
pub const LINES: [&str; 3] = [
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"mork","title":"mork usage","version":"1"},"capabilities":{"experimentalApi":false}}}"#,
    r#"{"jsonrpc":"2.0","method":"initialized"}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read"}"#,
];

/// A read is cut after this long, in real time (1.10.6).
pub const CUT: Duration = Duration::from_secs(20);
/// Reads are a minute apart, on the engine's clock.
pub const EVERY_MS: i64 = 60_000;

#[derive(Debug, Default)]
struct Slot {
    running: Option<Receiver<std::io::Result<String>>>,
    last_ms: Option<i64>,
    group: Arc<AtomicI32>,
}

#[derive(Debug, Default)]
pub struct Reader {
    codex: Slot,
    agy: Slot,
}

impl Reader {
    fn slot(&mut self, cli: Cli) -> &mut Slot {
        if cli == Cli::Agy { &mut self.agy } else { &mut self.codex }
    }
}

impl Engine {
    /// Takes each finished read's answer; starts the next of each when one is due.
    pub(super) fn read_limits(&mut self) {
        for cli in [Cli::Codex, Cli::Agy] {
            self.read_limits_of(cli);
        }
    }

    fn read_limits_of(&mut self, cli: Cli) {
        let slot = self.limits_read.slot(cli);
        if let Some(rx) = &slot.running {
            let answered = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => None,
                Err(TryRecvError::Empty) => return,
            };
            slot.running = None;
            if let Some(result) = answered {
                self.limits_answered(cli, result);
            }
            return;
        }
        let now = clock::now_ms();
        if slot.last_ms.is_some_and(|t| now - t < EVERY_MS)
            || !matches!(self.phase, Phase::Ready)
            || super::super::jobs::context::wall_unavailable(self.ctx.mode, &self.ctx.bwrap).is_some()
        {
            return;
        }
        slot.last_ms = Some(now);
        // agy runs walled alone, and reads only for a sign-in: no token, no read.
        if cli == Cli::Agy && (self.ctx.mode == WallMode::None || !self.agy_signed_in()) {
            return;
        }
        let spec = match self.limits_spec(cli, now) {
            Ok(spec) => spec,
            Err(e) => return self.log(&format!("{} limits: {e}", cli.as_str())),
        };
        let root = self.opts.home.root().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        let group = Arc::clone(&self.limits_read.slot(cli).group);
        let answer = if cli == Cli::Agy { Answer::AtExit } else { Answer::Rpc(&LINES) };
        std::thread::spawn(move || {
            let _ = tx.send(launch::walled_read(&spec, &root, answer, CUT, &group));
        });
        self.limits_read.slot(cli).running = Some(rx);
    }

    /// A clean shutdown ends the reads in flight: no process of them is left (T1.24).
    pub(super) fn end_limits_read(&mut self) {
        launch::end_group(&self.limits_read.codex.group);
        launch::end_group(&self.limits_read.agy.group);
    }

    // A seat can replace the token, so check its type through the logins root.
    fn agy_signed_in(&self) -> bool {
        crate::login_files::regular(&self.opts.home.logins(), Path::new(crate::login_files::AGY)).is_ok()
    }

    fn limits_answered(&mut self, cli: Cli, result: std::io::Result<String>) {
        let read = if cli == Cli::Agy { cli::limits::agy_read } else { cli::limits::codex_read };
        match result.map_err(|e| e.to_string()).and_then(|text| read(&text)) {
            Ok(windows) => {
                let limits = super::limits_of(cli, &windows, clock::now_ms());
                self.apply(&Event::LimitsSeen { limits, at: None, whole: Some(cli) });
            }
            Err(e) => self.log(&format!("{} limits: {e}", cli.as_str())),
        }
    }

    // Limits reads need the shared login outside any seat's home.
    fn limits_spec(&self, cli: Cli, now: i64) -> Result<mork_types::job::JobSpec, String> {
        let owner = AgentKey::new(format!("limits/{}", cli.as_str()));
        let id = mork_core::state::names::job_id(now, &owner, random::salt());
        let home = self.opts.home.login_home();
        let mut env = self.ctx.env(&id, &owner, None, &[], &self.opts.home.job(id.as_str()).join("io"));
        let argv: Vec<String> = match cli {
            Cli::Agy => {
                env.insert("AGY_CLI_DISABLE_AUTO_UPDATE".into(), "true".into());
                ["agy", "--output-format", "json", "-p=/usage"].map(str::to_owned).to_vec()
            }
            _ => vec!["codex".into(), "app-server".into()],
        };
        crate::seat_homes::env(&self.opts.home, &home, &mut env);
        let mut places = self.ctx.places(&home, Vec::new(), Vec::new(), None, &[])?;
        crate::seat_homes::login_places(&self.opts.home, &home, cli, &mut places).map_err(|e| e.to_string())?;
        Ok(self.ctx.spec(
            &id,
            mork_types::job::JobKind::Turn,
            &owner,
            argv,
            &home.display().to_string(),
            &places,
            env,
            Some(self.ctx.config.mork.ports.clone()),
            now,
        ))
    }
}
