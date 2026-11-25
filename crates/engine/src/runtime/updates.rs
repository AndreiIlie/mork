// Master's tip, read with the outside reads, owes a build (fac-19, `core::updates`); a build in place of a master
// commit past the pin is pinned, checked as `pin` is. Only a reload or a restart runs it.

use factory_core::decide::Event;
use factory_types::Sha;
use factory_types::view::PinLine;

use super::{Engine, Phase};
use crate::{builds, git};

impl Engine {
    /// A tip past the pin (else the running build) owes a build when it has none owed or made; one built before
    /// it reached master and never tried for the pin is tried now. Not while a reload is under way.
    pub(super) fn master_seen(&mut self) {
        if !matches!(self.phase, Phase::Ready) {
            return;
        }
        let Some(tip) = self.views_cache.tip.clone() else { return };
        let current = self.current();
        if tip == current {
            return;
        }
        if !factory_core::maintenance::updates::built(&self.state, &tip) {
            if self.behind(&current, &tip) {
                self.apply(&Event::MasterSeen { tip: Sha::new(tip), current, pinned: None });
            }
            return;
        }
        if factory_core::maintenance::updates::in_place(&self.state, &tip) && !self.pin_tried(&tip) {
            let pinned = self.update_pin(&tip);
            if pinned.is_some() {
                self.apply(&Event::MasterSeen { tip: Sha::new(tip), current, pinned });
            }
        }
    }

    // The pin, else the build running: what an update moves on from.
    fn current(&self) -> String {
        builds::pin(&self.opts.home).unwrap_or_else(|| self.opts.commit.clone())
    }

    // Whether `current` is `commit`'s ancestor in the factory repo: a build git can't place starts nothing.
    fn behind(&self, current: &str, commit: &str) -> bool {
        let Ok((repo, _)) = builds::factory_repo(&self.ctx.config) else { return false };
        git::descends(&self.opts.home, &repo, current, commit) == Some(true)
    }

    // A commit once pinned, or whose pin was tried, is never pinned by itself again: a pin moved
    // back by hand stays, and a refusal waits for its retry.
    fn pin_tried(&self, commit: &str) -> bool {
        self.store.pin_tried(commit).unwrap_or(true)
            || builds::moved_to_since(&self.opts.home, i64::MIN).iter().any(|c| c == commit)
    }

    /// A build of `commit` is in place: pinned when it is on master and the pin (else the running
    /// build) is behind it, so older work never moves the pin back. `None` when no pin is tried.
    pub(super) fn update_pin(&mut self, commit: &str) -> Option<Result<(), String>> {
        let (repo, base) = builds::factory_repo(&self.ctx.config).ok()?;
        let current = self.current();
        if current == commit
            || git::on_master(&self.opts.home, &repo, &base, commit) != Some(true)
            || !self.behind(&current, commit)
        {
            return None;
        }
        let home = &self.opts.home;
        let pinned = self.refusal(commit).and_then(|full| {
            builds::write_pin(home, &full, "factory")
                .map_err(|source| builds::BuildError::Io { what: "the pin".into(), source })
        });
        let pinned = pinned.map_err(|e| e.to_string());
        let line = match &pinned {
            Ok(()) => format!("pinned {commit}, the update"),
            Err(why) => format!("the update {commit} wasn't pinned: {why}"),
        };
        self.log(&line);
        Some(pinned)
    }

    /// The top line's pin: whether it waits for a reload, and master's tip on its way to it.
    pub(super) fn pin_line(&self, master: factory_types::view::Master) -> PinLine {
        let home = &self.opts.home;
        let pin = builds::pin(home);
        let ready = matches!(self.phase, Phase::Ready) && pin.as_ref().is_some_and(|p| *p != self.opts.commit);
        let current = self.current();
        let update = self.views_cache.tip.as_ref().and_then(|tip| {
            factory_core::maintenance::updates::line(&self.state, tip, &current, builds::manifest(home, tip).is_some())
        });
        PinLine { pin: pin.map(|p| p.chars().take(7).collect()), master, ready, update }
    }
}
