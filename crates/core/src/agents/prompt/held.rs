// What a planner's or foreman's session already holds of its prompt (fac-618, fac-620, fac-623): a resumed turn
// sends a fixed section, or the plan, only when its text differs from what the session was last sent.

use std::collections::BTreeMap;

use mork_types::reply::Role;
use mork_types::state::{Agent, Job};

use crate::decide::SessionSeen;
use crate::state::State;

/// A turn's sections as its prompt is written: which it sends, and the texts it stages for its job.
pub(crate) struct Sections<'a> {
    held: Option<&'a BTreeMap<String, String>>,
    on: bool,
    pub(crate) staged: BTreeMap<String, String>,
}

impl<'a> Sections<'a> {
    /// A turn of `a`: a resumed planner's or foreman's skips what its session holds; a new session sends everything.
    pub(crate) fn of(a: &'a Agent, resumed: bool) -> Self {
        let on = matches!(a.role, Role::Planner | Role::Foreman);
        Self { held: (on && resumed).then_some(&a.held), on, staged: BTreeMap::new() }
    }

    /// Everything sent, as at a session's start: a rerun's sections given again.
    pub(crate) fn all(a: &Agent) -> Self {
        Self::holding(a, None)
    }

    /// A planner's or foreman's turn in a session known to hold `held` (fac-626); none, everything sent.
    pub(crate) fn holding(a: &Agent, held: Option<&'a BTreeMap<String, String>>) -> Self {
        let on = matches!(a.role, Role::Planner | Role::Foreman);
        Self { held: held.filter(|_| on), on, staged: BTreeMap::new() }
    }

    /// Whether `text` is sent as the section `name`: unless the session holds it. Staged when sent.
    pub(crate) fn sends(&mut self, name: &str, text: &str) -> bool {
        if self.holds(name) == Some(text) {
            return false;
        }
        self.stage(name, text);
        true
    }

    /// The text the session holds as `name`, when this turn may rely on it.
    pub(crate) fn holds(&self, name: &str) -> Option<&'a str> {
        self.held?.get(name).map(String::as_str)
    }

    pub(crate) fn stage(&mut self, name: &str, text: &str) {
        if self.on {
            self.staged.insert(name.to_owned(), text.to_owned());
        }
    }
}

// `codex exec`'s stream shows no compaction, so its seat holds nothing and reads everything every turn.
fn tracks(job: &Job) -> bool {
    job.spec.live.is_some() || job.spec.argv.first().is_some_and(|w| w == mork_types::config::Cli::Agy.as_str())
}

/// A turn launched: one through `codex exec` leaves its session holding nothing a later turn may rely on.
pub(crate) fn launched(a: &mut Agent, job: &Job) {
    if !tracks(job) {
        a.held.clear();
    }
}

/// At settlement: a compacted session holds nothing it was sent before; a prompt its model answered is held.
pub(crate) fn seen(a: &mut Agent, job: &Job, s: &SessionSeen) {
    if s.compacted {
        compacted(a);
    } else if s.answered {
        a.needs_specification = false;
        if tracks(job) {
            a.held.extend(job.staged.clone());
        }
    }
}

pub(crate) fn compacted(a: &mut Agent) {
    a.ran.compacted = true;
    a.needs_specification = true;
    a.held.clear();
}

/// A cut turn's stream may have missed a compaction: clear held sections so the next turn resends them (fac-629).
pub(crate) fn cut(a: &mut Agent) {
    a.held.clear();
}

/// What a reply of `job` wrote itself, a recap or a mock offer, is held by its session.
pub(crate) fn wrote(a: &mut Agent, job: &Job, name: &str, text: &str) {
    if tracks(job) {
        a.held.insert(name.to_owned(), text.to_owned());
    }
}

/// Whether a rerun needs the fixed sections again. A retry of an exec prompt can replay its full sections,
/// while a correction starts with only the refusal.
pub(super) fn lost(a: &Agent, asked: Option<&Job>, codex_exec: bool, correction: bool) -> bool {
    matches!(a.role, Role::Planner | Role::Foreman)
        && a.session_started
        && (a.held.is_empty() || codex_exec)
        && asked.is_some_and(|job| correction || tracks(job))
}

/// The sections a session surely holds as `first`, its opening prompt, sent them (fac-626): one any later prompt, saved
/// or not, or the session's record gives other text may have been replaced, so it is left out.
pub(crate) fn certain(state: &State, a: &Agent, first: &Job) -> BTreeMap<String, String> {
    let since = |j: &&Job| j.owner == a.key && j.id != first.id && j.spec.created_ms >= first.spec.created_ms;
    let others: Vec<&BTreeMap<String, String>> =
        state.jobs.values().filter(since).map(|j| &j.staged).chain([&a.held]).collect();
    let mut held = first.staged.clone();
    held.retain(|name, text| others.iter().all(|o| o.get(name).is_none_or(|t| t == text)));
    held
}

/// The sections a retried turn's prompt sent, as its earlier job staged them.
pub(crate) fn retried(state: &State, a: &Agent, prompt: &str) -> BTreeMap<String, String> {
    let ran = state
        .jobs
        .values()
        .filter(|j| j.owner == a.key && !j.request_text().is_empty() && prompt.ends_with(j.request_text()));
    ran.max_by_key(|j| j.spec.created_ms).map(|j| j.staged.clone()).unwrap_or_default()
}
