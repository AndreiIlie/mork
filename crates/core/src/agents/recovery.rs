//! Recovery of a job the state doesn't record as settled (1.4.6): what the folder shows,
//! and what the engine does about it.

/// What the engine found of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Observed {
    pub folder: bool,
    pub claim: bool,
    pub lock_held: bool,
    pub exit: bool,
    /// `run.json` exists: the wall started.
    pub run: bool,
}

/// What the engine does about a job it found unsettled (1.4.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Crashed before writing the folder: write it from the intent and launch, same id.
    WriteAndLaunch,
    /// Crashed before the supervisor claimed it: start a supervisor, same id.
    StartSupervisor,
    /// A supervisor is about to claim it, or is just ending: wait for the lock.
    Wait,
    /// Running: reattach (1.4.7).
    Reattach,
    /// Ended while no one watched: take its result and apply it once.
    TakeResult,
    /// Its supervisor died with no end: end what is left of the job, then apply a complete
    /// result once, else record it cut.
    EndThenResultOrCut,
    /// Any other combination: recorded cut, with what was found in the log.
    Cut,
    /// Never launched, and its stop is recorded: settled stopped, never launched (1.4.6.2).
    SettleStopped,
}

/// The recovery for what was found (1.4.6). A recorded stop wins: a job never launched is settled stopped.
pub fn recover(o: Observed, stop_recorded: bool) -> Recovery {
    let r = table(o);
    if stop_recorded && matches!(r, Recovery::WriteAndLaunch | Recovery::StartSupervisor) {
        Recovery::SettleStopped
    } else {
        r
    }
}

fn table(o: Observed) -> Recovery {
    match (o.folder, o.claim, o.lock_held, o.exit) {
        (false, _, _, _) => Recovery::WriteAndLaunch,
        (true, false, false, false) => Recovery::StartSupervisor,
        (true, false, true, _) => Recovery::Wait,
        (true, true, true, false) => Recovery::Reattach,
        (true, true, true, true) => Recovery::Wait,
        (true, true, false, true) => Recovery::TakeResult,
        (true, true, false, false) => Recovery::EndThenResultOrCut,
        (true, false, false, true) => Recovery::Cut,
    }
}

/// How a job ended (1.4.6, 1.4.7): a recorded stop wins over anything; then a complete result; then an exit with no
/// result is a failed turn; no exit at all is a cut.
pub fn end_of(
    stop_recorded: bool,
    exit: Option<(bool, Option<String>)>,
    result: Option<Result<serde_json::Value, String>>,
    session: crate::decide::SessionSeen,
    cli_words: impl FnOnce() -> String,
) -> crate::decide::JobEnd {
    use crate::decide::JobEnd;
    let exit_stopped = exit.as_ref().is_some_and(|(stopped, _)| *stopped);
    if stop_recorded || exit_stopped {
        return JobEnd::Stopped;
    }
    match (result, exit) {
        (Some(reply), _) => JobEnd::Finished { reply, session },
        // A stream that showed a reached limit ends as a failed turn with its session, so the
        // reset rule (1.4.10.2) sees it (M2.3).
        (None, Some((_, error))) if session.limit_reached => {
            JobEnd::Finished { reply: Err(error.unwrap_or_else(cli_words)), session }
        }
        (None, Some((_, error))) => JobEnd::Failed { words: error.unwrap_or_else(cli_words) },
        (None, None) => JobEnd::Cut { session },
    }
}
