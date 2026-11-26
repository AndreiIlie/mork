// M5.36: the window redraws on its own clock only while an icon spins or a hand blinks.

use mork_types::seen::StepStatus;
use mork_types::view::{CacheState, Doing};

use crate::app::App;
use crate::session::Tab;

impl App {
    /// The window’s clock moved, for animations and countdowns (5.11).
    pub fn tick(&mut self, now_ms: i64) {
        self.now_ms = now_ms;
    }

    /// The frame interval in ms while a shown working icon or step mark turns (fac-128) or a hand
    /// blinks (fac-110), or usage counts down: an overlay hides the turning ones.
    pub fn frame_every(&self) -> Option<i64> {
        // The shortest wins: a quarter-minute countdown must not slow a timer counting seconds.
        [self.animation_frame(), self.countdown_frame(), self.timer_frame()].into_iter().flatten().min()
    }

    fn animation_frame(&self) -> Option<i64> {
        if !self.settings.animations {
            return None;
        }
        let blinking =
            self.raised.values().any(|seen| (0..crate::hands::BLINK_MS).contains(&self.now_ms.saturating_sub(*seen)));
        let agents_shown = self.overlay.is_none() && (self.session.place != "main" || self.session.tab == Tab::Engine);
        let working = agents_shown
            && self.place().is_some_and(|v| v.agents.iter().any(|a| matches!(a.doing, Doing::Working { .. })));
        let listed = self
            .place()
            .is_some_and(|v| v.lists.iter().any(|l| l.steps.iter().any(|s| s.status == StepStatus::InProgress)));
        let turning = self.overlay.is_none() && (listed || crate::plan::turning(self));
        if working || turning { Some(25) } else { blinking.then_some(50) }
    }

    // fac-206: counts to the second, animations or not.
    fn timer_frame(&self) -> Option<i64> {
        let chat = self.overlay.is_none() && (self.session.place != "main" || self.session.tab == Tab::Engine);
        let counting = |t: &mork_types::chat::ChatTurn| {
            t.running
                && (t.turn.thinking_since.is_some()
                    || t.turn.steps.iter().any(|s| s.start_ms.is_some() && s.end_ms.is_none()))
        };
        (chat && self.place().is_some_and(|v| v.turns.iter().any(counting))).then_some(250)
    }

    // Ticks animations or not: each second while one counts seconds, else each quarter minute.
    fn countdown_frame(&self) -> Option<i64> {
        let usage = &self.screen.top.usage;
        if crate::spans::counts_seconds(usage, self.now_ms) {
            Some(250)
        } else if usage.iter().any(|s| crate::spans::reset_in(s, self.now_ms).is_some()) || self.warm_cache_shown() {
            Some(15_000)
        } else {
            None
        }
    }

    // Idle caches still above expiry need periodic redraws so Context and Raised Hands update on time (fac-670).
    fn warm_cache_shown(&self) -> bool {
        let hand = crate::hands::of(self).into_iter().any(|h| h.cold_at_ms.is_some_and(|at| at > self.now_ms));
        let session = self.place().is_some_and(|v| {
            v.sessions
                .iter()
                .any(|a| matches!(a.cache, Some(CacheState::Idle { cold_at_ms }) if cold_at_ms > self.now_ms))
        });
        hand || session
    }
}
