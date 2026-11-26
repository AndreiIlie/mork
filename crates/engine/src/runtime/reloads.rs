// A reload's last steps in the old app (1.8.6.4 and .5): `pin.next`, then the exec of the target,
// with the lock and the listeners kept open.

use mork_core::decide::Event;
use std::os::fd::AsRawFd;

use super::startup::INHERIT;
use super::{Engine, Phase};
use crate::builds::{self, PinNext};
use crate::jobs::launch;
use crate::{clock, files};

impl Engine {
    /// When a reload's hold is over, exec its target.
    pub(super) fn reload_due(&mut self) {
        let Phase::Reloading { target, id, at, who } = self.phase.clone() else { return };
        if clock::instant() < at {
            return;
        }
        // Outward work under way gets up to 10 s to finish and be applied; what still runs then
        // is left to recovery (1.8.6.3, M5.17).
        if !self.pipe.in_flight.is_empty() && clock::instant() < at + std::time::Duration::from_secs(10) {
            return;
        }
        let next = PinNext { target: target.clone(), id: id.clone(), from: self.opts.commit.clone(), who };
        if let Err(e) = files::write_json(&self.opts.home.pin_next(), &next) {
            self.reload_failed(&format!("pin.next couldn't be written: {e}"));
            return;
        }
        // `ui-session.json` is the window's, kept current as it changes: left as it is (M4.7).
        self.end_limits_read();
        self.hooks_settle();
        self.previews_end();
        self.wakes_cut_for_reload();
        self.mocks_end();
        self.wall_runs_end();
        // Batches written during preview or mock teardown settle before exec (fac-665).
        self.settle_waiting_wakes();
        let fds = [self.keep.lock.as_raw_fd(), self.keep.mork, self.keep.agent];
        for fd in fds {
            if let Err(e) = mork_sys::keep_across_exec(fd) {
                self.reload_failed(&format!("a descriptor couldn't be kept: {e}"));
                return;
            }
        }
        let inherit = fds.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        let mut args = vec!["--reloaded".to_owned(), id];
        args.extend(self.opts.mode_args.clone());
        let err = launch::exec(&builds::binary(&self.opts.home, &target), &args, &[(INHERIT.to_owned(), inherit)]);
        // The exec failed: the kept descriptors close on the next exec again (M1.3).
        for fd in fds {
            let _ = mork_sys::close_on_exec(fd);
        }
        self.reload_failed(&format!("the exec of {} failed: {err}", &target[..7.min(target.len())]));
    }

    // The old app carries on and says why (1.8.6.7).
    fn reload_failed(&mut self, why: &str) {
        let _ = std::fs::remove_file(self.opts.home.pin_next());
        self.phase = Phase::Ready;
        self.wakes_resume();
        // The actions decided meanwhile were left pending: they run now.
        self.run_pending();
        self.log(why);
        self.apply(&Event::EngineSays { place: "main".into(), text: format!("The reload failed: {why}"), key: None });
    }
}
