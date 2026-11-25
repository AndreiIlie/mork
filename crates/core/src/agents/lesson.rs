//! What a closing turn asks (fac-176, fac-287, fac-592): any feedback the seat didn't file, about its project and
//! about the factory, either of which may be nothing.

use factory_types::reply::{FACTORY_FEEDBACK_ANSWER, FEEDBACK_ANSWER};
use factory_types::state::Agent;
use serde_json::Value;

const QUESTION: &str = "Before you go: did you meet a bug, an issue, or have a suggestion that you haven't filed with `factory ctl notes.add`? If it is about this project, say it in `feedback`: the engine files it as a note to the project's orchestrator. If it is about the factory itself, say it in `factory_feedback`: the engine files it as a note to the factory's orchestrator, whichever project your card is on. On the factory's own card both go to the factory's notes. Say what happened and what would help. \"nothing\" in either where there is none.";

/// A session long enough to have learned something: more than two turns, or at least one since its
/// latest answered closing turn (fac-670), with a tool call. A turn `running` counts until settled.
pub(super) fn owed(a: &Agent, running: bool) -> bool {
    let turns = a.ran.turns.saturating_add(u32::from(running));
    let enough = if a.ran.closed > 0 { turns > a.ran.closed } else { turns > 2 };
    a.session_started && enough && (a.ran.tools || running)
}

pub(super) fn prompt() -> String {
    format!(
        "Your seat is ending. Nothing you answer moves the card or reaches another agent.\n\n{QUESTION}\n\nReply with one JSON object, as this turn's schema says: `{FEEDBACK_ANSWER}` and `{FACTORY_FEEDBACK_ANSWER}`; no hand, no messages, no outcome."
    )
}

/// A refused closing reply's one correction.
pub(super) fn refused(reason: &str) -> String {
    format!(
        "Your closing reply was refused: {reason}. Reply again with your two answers, `{FEEDBACK_ANSWER}` (about this project, filed as a note to its orchestrator) and `{FACTORY_FEEDBACK_ANSWER}` (about the factory, filed as a note to the factory's orchestrator), each \"nothing\" where there is none, fixed."
    )
}

/// A closing reply's answers: the project's feedback and the factory's, each unless it is nothing.
pub(super) fn check(reply: &Value) -> Result<(Option<String>, Option<String>), String> {
    let obj = reply.as_object().ok_or("the reply isn't an object")?;
    if let Some(k) = obj.keys().find(|k| ![FEEDBACK_ANSWER, FACTORY_FEEDBACK_ANSWER].contains(&k.as_str())) {
        return Err(format!("`{k}` isn't one of the closing turn's answers"));
    }
    let text = |k: &str| match obj.get(k).and_then(Value::as_str).map(str::trim) {
        Some(t) if !t.is_empty() => Ok((!nothing(t)).then(|| t.to_owned())),
        Some(_) => Err(format!("`{k}` is blank")),
        None => Err(format!("`{k}` must be a text")),
    };
    Ok((text(FEEDBACK_ANSWER)?, text(FACTORY_FEEDBACK_ANSWER)?))
}

fn nothing(t: &str) -> bool {
    t.trim_end_matches('.').eq_ignore_ascii_case("nothing")
}
