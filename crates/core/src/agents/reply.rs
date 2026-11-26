//! A refused reply posts no effects: malformed JSON, wrong-role outcomes, incomplete findings, untyped messages,
//! invalid images, or ineffective foreman outcomes leaving a card waiting (fac-423). Echo replies hold one message.

use mork_types::card::{Mode, Stage};
use mork_types::config::Cli;
use mork_types::reply::{Outcome, Reply, Role};
use mork_types::state::{Agent, Job};
use serde_json::Value;

use crate::state::State;

// A refusal quotes no more of the CLI's words than this, in characters: they can be a whole answer.
const QUOTED: usize = 300;

/// Parses a role's reply and checks its rules, its outcome among those its job's saved schema offered; then scrubs it.
pub(super) fn check(role: Role, value: &Value, job: &Job) -> Result<Reply, String> {
    let mut r = parse(value)?;
    clean(&mut r);
    rules(role, &allowed(role, job), &r)?;
    Ok(r)
}

/// A seat's reply schema for its next turn: a closing turn's feedback answers (fac-287, fac-592, fac-725), or one offering the outcomes it may end with now.
pub fn schema_of(state: &State, a: &Agent, cli: Cli, with_mork: bool) -> Value {
    if super::closing::ending_turn(a) {
        return mork_types::reply::closing_schema(with_mork);
    }
    mork_types::reply::schema_offering(a.role, super::children::is_child(a), cli, &outcomes(state, a))
}

/// The outcomes an agent's turn may end with now: its role's, and `brief-ready` for a no-planner card's room foreman
/// at brief (past any baseline step) or while a reopened mock is open (fac-616, fac-688).
pub fn outcomes(state: &State, a: &Agent) -> Vec<Outcome> {
    let mut all = a.role.outcomes().to_vec();
    let recaps = |c: &mork_types::card::Card| {
        c.no_planner
            && c.mode == Mode::Full
            && ((c.stage == Stage::Brief && !crate::lifecycle::mock::baseline(c)) || crate::lifecycle::mock::open(c))
    };
    if a.role == Role::Foreman && a.parent.is_none() && state.cards.get(a.place.as_str()).is_some_and(recaps) {
        all.insert(1, Outcome::BriefReady);
    }
    all
}

// The turn may end with `brief-ready` only if its saved schema offered it: the card may have left brief since.
fn allowed(role: Role, job: &Job) -> Vec<Outcome> {
    let mut all = role.outcomes().to_vec();
    let saved = job.io_files.get("schema.json").and_then(|s| serde_json::from_str::<Value>(s).ok());
    if role == Role::Foreman && saved.is_some_and(|s| mork_types::reply::offers(&s, Outcome::BriefReady)) {
        all.insert(1, Outcome::BriefReady);
    }
    all
}

// A CLI's answer that wasn't JSON comes as the CLI's words on it, a string.
fn parse(value: &Value) -> Result<Reply, String> {
    if let Value::String(words) = value {
        let quoted: String = words.chars().take(QUOTED).collect();
        return Err(format!("the reply doesn't parse: {quoted}"));
    }
    serde_json::from_value(value.clone()).map_err(|e| format!("the reply doesn't parse: {e}"))
}

fn rules(role: Role, outcomes: &[Outcome], r: &Reply) -> Result<(), String> {
    if !outcomes.contains(&r.outcome) {
        let allowed: Vec<&str> = outcomes.iter().map(|o| o.as_str()).collect();
        return Err(format!(
            "the outcome `{}` isn't one of the {}'s ({})",
            r.outcome.as_str(),
            role.as_str(),
            allowed.join(", ")
        ));
    }
    for (i, f) in r.findings.iter().enumerate() {
        let n = i + 1;
        if f.file.is_empty() {
            return Err(format!("finding {n} names no file"));
        }
        if f.line < 1 {
            return Err(format!("the finding in {} has line {}, not a whole number from 1", f.file, f.line));
        }
        if f.proof.is_empty() {
            return Err(format!("the finding at {}:{} has no proof", f.file, f.line));
        }
    }
    if let Some((i, m)) = r.messages.iter().enumerate().find(|(_, m)| m.kind.is_none()) {
        return Err(format!("message {}, to `{}`, has no kind: request, response or update", i + 1, m.to));
    }
    Ok(())
}

// Removes terminal control characters and bidirectional overrides, and trims (3.5.3).
fn scrub(s: &str) -> String {
    let kept: String = s
        .chars()
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'))
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    kept.trim().to_owned()
}

fn clean(r: &mut Reply) {
    let h = &mut r.hand;
    for s in [&mut h.headline, &mut h.message, &mut h.reason] {
        *s = scrub(s);
    }
    for s in h.options.iter_mut().chain(&mut h.changes).chain(&mut h.code_shape) {
        *s = scrub(s);
    }
    for m in &mut r.messages {
        m.to = scrub(&m.to);
        m.text = scrub(&m.text);
    }
    for f in &mut r.findings {
        for s in [&mut f.file, &mut f.summary, &mut f.proof] {
            *s = scrub(s);
        }
    }
}

/// The echo role's reply: `{"message": "<text>"}`; its one effect is that message (1.10.4).
pub(super) fn check_echo(reply: &Value) -> Result<String, String> {
    let obj = reply.as_object().ok_or("the reply isn't an object")?;
    if obj.len() != 1 {
        return Err("the reply must hold exactly `message`".into());
    }
    match obj.get("message").and_then(Value::as_str) {
        Some(t) if !t.trim().is_empty() => Ok(t.trim().to_owned()),
        Some(_) => Err("`message` is blank".into()),
        None => Err("`message` must be a string".into()),
    }
}
