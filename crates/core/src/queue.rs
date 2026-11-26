//! The merge queue (fac-10), a merge train since fac-68: cards enter at the back once their gates pass, each
//! candidate is made and run, and only the front lands; the test slots are leased here too.

pub mod approval;
pub mod candidates;
pub mod checkout;
pub mod entries;
pub mod failing;
pub mod holds;
pub mod joins;
pub mod landing;
pub mod leases;
pub mod members;
pub mod order;
pub mod own_runs;
pub mod pr;
pub mod pr_merged;
pub mod recovery;
pub mod refresh;
pub mod runs;
pub mod suite_log;
pub mod suite_news;
pub mod train;
pub mod wall_runs;

use mork_types::Command;
use mork_types::state::Sender;

use crate::decide::{Ctx, Decision};
use crate::state::State;

/// `merge`, or a test slot's command (`test.lease`, `test.resume`, `test.done`), past the buzz.
pub fn command(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    if let Command::Merge { card } = cmd {
        members::admit(state, &card.as_str().to_lowercase(), ctx)
    } else if let Command::TestLease { kind, command } = cmd {
        leases::lease(state, from, kind, command, ctx)
    } else if let Command::TestResume { lease } = cmd {
        leases::resume(state, from, lease, ctx)
    } else if let Command::TestDone { lease, exit } = cmd {
        leases::done(state, from, lease, *exit, ctx)
    } else {
        Decision::refused(format!("`{}` isn't the queue's command", cmd.name()))
    }
}
