// A retry or a correction runs its turn's prompt again (fac-18), with what changed since it was asked: today's
// contract and, after a compaction, every standing section (fac-620).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mork_types::JobId;
use mork_types::config::Config;
use mork_types::state::{Agent, Job};

use super::held::{self, Sections};
use super::specification::{FOREMAN_SPECIFICATION, planner_foreman};
use super::{card_of, fixed, reply_reminder};
use crate::maintenance::manuals::CONTRACT;
use crate::state::State;

/// The turn a rerun asks again: a retry's, its session's opening request when named (fac-626), else the agent's last
/// settled one; or the job a correction corrects.
#[derive(Clone, Copy)]
pub(in crate::agents) enum Asked<'j> {
    Retry(Option<&'j JobId>),
    Correction(&'j JobId),
}

/// A retry's or a correction's prompt (fac-18): it runs under this build's reply schema, so a turn asked under another
/// is given today's contract.
pub(in crate::agents) fn rerun(
    state: &State,
    a: &Agent,
    config: &Config,
    (asked, resume, codex_exec): (Asked<'_>, bool, bool),
    asked_prompt: &str,
) -> (String, BTreeMap<String, String>) {
    let last = || state.jobs.values().filter(|j| j.owner == a.key && j.settled.is_some()).max_by_key(|j| j.settled_ms);
    let (of, correction) = match asked {
        Asked::Retry(of) => (of, false),
        Asked::Correction(of) => (Some(of), true),
    };
    let asked = of.map_or_else(last, |j| state.jobs.get(j));
    let restore = resume && held::lost(a, asked, codex_exec, correction);
    let prompt = if restore { without_again(asked_prompt) } else { asked_prompt };
    let mut text = asked.and_then(contract_since).map_or_else(|| prompt.to_owned(), |c| format!("{prompt}\n\n{c}"));
    let mut s = Sections::all(a);
    // A session that lost its sections, or launches through `codex exec`, reads them again, the challenge's plan only
    // if its turn read it.
    if restore {
        let plan =
            card_of(state, a).zip(asked).is_some_and(|(c, j)| crate::agents::required::bound(c, &a.key, j).is_some());
        text.push_str(AGAIN);
        fixed(state, a, config, (&mut s, plan), &mut text);
        reply_reminder(state, a, &mut s, &mut text);
    }
    if planner_foreman(state, a) && !text.contains(FOREMAN_SPECIFICATION) {
        let _ = write!(text, "\n\n{FOREMAN_SPECIFICATION}");
    }
    // A retry's session holds what its earlier prompt sent, unless it lost it: then only what is given again counts.
    let mut staged = if restore || correction { BTreeMap::new() } else { held::retried(state, a, asked_prompt) };
    staged.append(&mut s.staged);
    (text, staged)
}

const CONTINUE: &str = "Your turn ended before its reply; this one runs it again. Its request, the prompt that began this session, is already in it word for word and is not sent again. Go on with it from where you stopped, and reply to it.";

/// A short retry's prompt and the sections it stages (fac-626): the request its session holds, continued, with the
/// cut note when it was cut, today's contract, the standing sections changed since, the reply's reminder and guidance.
pub(in crate::agents) fn continuation(
    state: &State,
    a: &Agent,
    config: &Config,
    (request, codex_exec): (&Job, bool),
    cut: bool,
) -> (String, BTreeMap<String, String>) {
    let mut text = String::new();
    if cut {
        let _ = write!(text, "{}\n\n", crate::agents::turns::CUT_NOTE);
    }
    let _ = writeln!(text, "{CONTINUE}");
    if let Some(c) = contract_since(request) {
        let _ = writeln!(text, "\n{c}");
    }
    // Its session holds what the request's prompt sent and no later one may have replaced, read so at launch; through
    // `codex exec` all is sent (fac-620).
    let certain = held::certain(state, a, request);
    let mut s = Sections::holding(a, (!codex_exec).then_some(&certain));
    let plan = card_of(state, a).is_some_and(|c| crate::agents::required::bound(c, &a.key, request).is_some());
    fixed(state, a, config, (&mut s, plan), &mut text);
    reply_reminder(state, a, &mut s, &mut text);
    let mut sent = s.staged;
    let mut staged = certain;
    staged.append(&mut sent);
    (text.trim_end().to_owned(), staged)
}

const AGAIN: &str = "\n\n# Your standing sections, again\n\nYour session may no longer hold the sections it was given, as after a compaction of its context: here they are as they stand.\n";

// A rerun's repeated sections replace the ones an earlier rerun added (fac-620).
fn without_again(prompt: &str) -> &str {
    prompt.rsplit_once(AGAIN).map_or(prompt, |(original, _)| original)
}

// The contract it runs under now, replacing the one the turn was asked with; none for this schema's.
fn contract_since(asked: &mork_types::state::Job) -> Option<String> {
    let schema = asked.io_files.get("schema.json").and_then(|s| serde_json::from_str(s).ok())?;
    let (was, now) = (mork_types::reply::version_of(&schema), mork_types::reply::SCHEMA_VERSION);
    (was != now).then(|| {
        format!(
            "The reply contract changed after this turn was first asked, under reply schema {was}; this turn's is {now}. Its whole text replaces the one you were given:\n\n{CONTRACT}\n\n`schema` is {now}."
        )
    })
}
