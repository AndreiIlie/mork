//! The home's layout (1.3).

use std::path::{Path, PathBuf};

/// One Mork home folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `$MORK_HOME`, else `$XDG_DATA_HOME/mork`, else `~/.local/share/mork`.
    pub fn from_env() -> Option<Self> {
        if let Some(h) = std::env::var_os("MORK_HOME").filter(|v| !v.is_empty()) {
            return Some(Self::new(h));
        }
        if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            return Some(Self::new(PathBuf::from(x).join("mork")));
        }
        let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
        Some(Self::new(PathBuf::from(home).join(".local/share/mork")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn app_lock(&self) -> PathBuf {
        self.root.join("app.lock")
    }
    pub fn mork_sock(&self) -> PathBuf {
        self.root.join("mork.sock")
    }
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }
    pub fn agent_sock(&self) -> PathBuf {
        self.run_dir().join("agent.sock")
    }
    pub fn state_db(&self) -> PathBuf {
        self.root.join("state.db")
    }
    pub fn jobs(&self) -> PathBuf {
        self.root.join("jobs")
    }
    pub fn job(&self, id: &str) -> PathBuf {
        self.jobs().join(id)
    }
    /// The seats' own `io/` folders (fac-654), each its `$MORK_IO` for its lifetime.
    pub fn seat_ios(&self) -> PathBuf {
        self.root.join("io")
    }
    /// The seat's `io/` folder, named by its first turn's job id (`Agent::io`).
    pub fn seat_io(&self, id: &str) -> PathBuf {
        self.seat_ios().join(id)
    }
    pub fn homes(&self) -> PathBuf {
        self.root.join("homes")
    }
    pub fn seat_home(&self, io: &str) -> PathBuf {
        self.homes().join(&crate::files::sha256_hex(io.as_bytes())[..16])
    }
    pub fn login_home(&self) -> PathBuf {
        self.root.join("login-home")
    }
    pub fn logins(&self) -> PathBuf {
        self.root.join("logins")
    }
    pub fn caches(&self) -> PathBuf {
        self.root.join("caches")
    }
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }
    pub fn builds(&self) -> PathBuf {
        self.root.join("builds")
    }
    /// The installed browsers (fac-114): read-only in every wall, put there outside Mork.
    pub fn browsers(&self) -> PathBuf {
        self.root.join("browsers")
    }
    /// The orchestrators' queued wake lines (fac-340): kept so a reload doesn't lose them.
    pub fn wakes(&self) -> PathBuf {
        self.root.join("wakes.json")
    }
    pub fn pin(&self) -> PathBuf {
        self.root.join("pin")
    }
    pub fn pin_next(&self) -> PathBuf {
        self.root.join("pin.next")
    }
    /// A pin move under way (M1.15): reconciled at the next start when a crash cut it short.
    pub fn pin_move(&self) -> PathBuf {
        self.root.join("pin.move")
    }
    pub fn pins_log(&self) -> PathBuf {
        self.root.join("pins.log")
    }
    pub fn agent_home(&self) -> PathBuf {
        self.root.join("agent-home")
    }
    /// The orchestrator waker's home and working folder (fac-265): in no seat's wall.
    pub fn waker_home(&self) -> PathBuf {
        self.root.join("waker-home")
    }
    pub fn clones(&self) -> PathBuf {
        self.root.join("clones")
    }
    pub fn mirrors(&self) -> PathBuf {
        self.root.join("mirrors")
    }
    /// The merge train's clones, one per candidate, `<card>-<tag>` (fac-68).
    pub fn trains(&self) -> PathBuf {
        self.root.join("trains")
    }
    /// A review card's handoff, set aside while its clone is made again (fac-287).
    pub fn handoffs(&self) -> PathBuf {
        self.root.join("handoffs")
    }
    /// A card's mock, kept from its removed clone for its next run's (fac-591).
    pub fn mocks(&self) -> PathBuf {
        self.root.join("mocks")
    }
    /// A review card's proofs, set aside while its clone is made again (fac-416).
    pub fn proofs(&self) -> PathBuf {
        self.root.join("proofs")
    }
    pub fn records(&self) -> PathBuf {
        self.root.join("records")
    }
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
    }
    /// The images pasted into message drafts, kept for the messages that carry them.
    pub fn pasted(&self) -> PathBuf {
        self.root.join("pasted")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// The door's targets (fac-1): engine-only, never in a wall.
    pub fn targets(&self) -> PathBuf {
        self.root.join("targets.toml")
    }
    /// The door's engine-only material (keys, pinned host keys, local credentials) and its
    /// calls' markers and receipts.
    pub fn door(&self) -> PathBuf {
        self.root.join("door")
    }
    /// The local web bridge's owner-only secret key (fac-671): never in a wall.
    pub fn web_key(&self) -> PathBuf {
        self.root.join("web.key")
    }
}
