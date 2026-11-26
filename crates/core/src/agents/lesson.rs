//! What a closing turn asks (fac-176, fac-287, fac-592): any feedback the seat didn't file, about its project and
//! about Mork, either of which may be nothing.

use mork_types::reply::{FEEDBACK_ANSWER, MORK_FEEDBACK_ANSWER};
use mork_types::state::Agent;
use serde_json::Value;

const QUESTION: &str = "Before you go: did you meet a bug, an issue, or have a suggestion that you haven't filed with `mork ctl notes.add`? If it is about this project, say it in `feedback`: the engine files it as a note to the project's orchestrator. If it is about Mork itself, say it in `mork_feedback`: the engine files it as a note to Mork's orchestrator, whichever project your card is on. On Mork's own card both go to Mork's notes. Say what happened and what would help. \"nothing\" in either where there is none.";

const QUESTION_PROJECT_ONLY: &str = "Before you go: did you meet a bug, an issue, or have a suggestion that you haven't filed with `mork ctl notes.add`? Say it in `feedback`: the engine files it as a note to the project's orchestrator. Say what happened and what would help. \"nothing\" where there is none.";

/// A session long enough to have learned something: more than two turns, or at least one since its
/// latest answered closing turn (fac-670), with a tool call. A turn `running` counts until settled.
pub(super) fn owed(a: &Agent, running: bool) -> bool {
    let turns = a.ran.turns.saturating_add(u32::from(running));
    let enough = if a.ran.closed > 0 { turns > a.ran.closed } else { turns > 2 };
    a.session_started && enough && (a.ran.tools || running)
}

pub(super) fn prompt(with_mork: bool) -> String {
    if with_mork {
        format!(
            "Your seat is ending. Nothing you answer moves the card or reaches another agent.\n\n{QUESTION}\n\nReply with one JSON object, as this turn's schema says: `{FEEDBACK_ANSWER}` and `{MORK_FEEDBACK_ANSWER}`; no hand, no messages, no outcome."
        )
    } else {
        format!(
            "Your seat is ending. Nothing you answer moves the card or reaches another agent.\n\n{QUESTION_PROJECT_ONLY}\n\nReply with one JSON object, as this turn's schema says: `{FEEDBACK_ANSWER}`; no hand, no messages, no outcome."
        )
    }
}

/// A refused closing reply's one correction.
pub(super) fn refused(reason: &str, with_mork: bool) -> String {
    if with_mork {
        format!(
            "Your closing reply was refused: {reason}. Reply again with your two answers, `{FEEDBACK_ANSWER}` (about this project, filed as a note to its orchestrator) and `{MORK_FEEDBACK_ANSWER}` (about Mork, filed as a note to Mork's orchestrator), each \"nothing\" where there is none, fixed."
        )
    } else {
        format!(
            "Your closing reply was refused: {reason}. Reply again with your answer, `{FEEDBACK_ANSWER}` (about this project, filed as a note to its orchestrator), \"nothing\" where there is none, fixed."
        )
    }
}

/// A closing reply's answers: the project's feedback and, when configured, Mork's, each unless it is nothing.
pub(super) fn check(reply: &Value, with_mork: bool) -> Result<(Option<String>, Option<String>), String> {
    let obj = reply.as_object().ok_or("the reply isn't an object")?;
    let allowed: &[&str] = if with_mork { &[FEEDBACK_ANSWER, MORK_FEEDBACK_ANSWER] } else { &[FEEDBACK_ANSWER] };
    if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("`{k}` isn't one of the closing turn's answers"));
    }
    let text = |k: &str| match obj.get(k).and_then(Value::as_str).map(str::trim) {
        Some(t) if !t.is_empty() => Ok((!nothing(t)).then(|| t.to_owned())),
        Some(_) => Err(format!("`{k}` is blank")),
        None => Err(format!("`{k}` must be a text")),
    };
    let mork = if with_mork { text(MORK_FEEDBACK_ANSWER)? } else { None };
    Ok((text(FEEDBACK_ANSWER)?, mork))
}

fn nothing(t: &str) -> bool {
    t.trim_end_matches('.').eq_ignore_ascii_case("nothing")
}
