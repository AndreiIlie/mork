//! Waits on what the app has done, where a fixed sleep once guessed how long it takes: each on
//! a state the test can read, failing the test when it doesn't come in time.

use std::time::Duration;

use serde_json::json;

use super::{Rig, wait_until};

const LONG: Duration = Duration::from_secs(40);

impl Rig {
    /// Waits until the app's loop has taken every command sent before now and gone round once more: it takes
    /// its inputs in order and does a tick's work after each.
    pub fn wait_loop(&self) {
        for _ in 0..2 {
            let a = self.send(json!({"status": {}}));
            assert!(a.get("ok").is_some(), "{a}");
        }
    }

    /// Waits until no outward action is pending or under way: each one decided has run (2.5).
    pub fn wait_actions_done(&self) {
        use mork_types::action::ActionStatus;
        wait_until("every action run", LONG, || {
            self.state().actions.values().all(|a| !matches!(a.status, ActionStatus::Pending | ActionStatus::Started))
        });
    }
}

/// Fails at once when no wall could start for the app's work, a wall inside a card's wall included (1.4.11.6):
/// the turns a wait is for would never come, and it would time out saying nothing (fac-73).
pub fn no_wall(state: &mork_core::state::State) {
    use mork_types::state::{AgentStatus, Settlement};
    let job = state.jobs.values().find_map(|j| match &j.settled {
        Some(Settlement::Failed { words }) if words.starts_with("the wall didn't start") => {
            Some(format!("{}'s job {}: {words}", j.owner, j.id))
        }
        _ => None,
    });
    let agent = || {
        state.agents.iter().find_map(|(key, a)| match &a.status {
            AgentStatus::Error { words } if words.starts_with("no wall:") => Some(format!("{key}: {words}")),
            _ => None,
        })
    };
    if let Some(why) = job.or_else(agent) {
        panic!(
            "{why}\nno wall can start here: inside a card's wall, run the app's tests with \
             `mork test --run cargo test -p mork-app --test <file>`, which runs them with no inner wall (AGENTS.md)"
        );
    }
}
