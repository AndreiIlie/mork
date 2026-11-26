//! What a reload brought (fac-75): the cards landed between two builds, each by its headline,
//! and the commits no card landed, by their short hash and subject.

use std::collections::{BTreeMap, BTreeSet};

use mork_types::action::{Action, ActionStatus, ActionWhat};
use serde_json::Value;

/// A commit the new build has and the old one hasn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: String,
}

/// A `card.landed` line with the base before and after the landing, where each is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    pub card: String,
    pub head: String,
    pub title: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// The base before and after a landing, from its row and the base move or merge that completed with it.
pub fn bounds<'a>(
    data: &Value,
    actions: impl IntoIterator<Item = &'a Action>,
    card: &str,
    at: i64,
) -> (Option<String>, Option<String>) {
    let s = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_owned);
    let (before, after) = (s("before"), s("after"));
    if before.is_some() && after.is_some() {
        return (before, after);
    }
    let (kept_before, kept_after) = kept(actions, card, at).unwrap_or_default();
    let before = if after.is_some() && after != kept_after { before } else { before.or(kept_before) };
    (before, after.or(kept_after))
}

fn kept<'a>(
    actions: impl IntoIterator<Item = &'a Action>,
    card: &str,
    at: i64,
) -> Option<(Option<String>, Option<String>)> {
    let landing = |a: &&Action| {
        a.status == ActionStatus::Done
            && a.done_ms == Some(at)
            && matches!(&a.what, ActionWhat::MoveBase { card: c, .. } | ActionWhat::Merge { card: c, .. } if c.as_str() == card)
    };
    let mut done = actions.into_iter().filter(landing);
    let (Some(a), None) = (done.next(), done.next()) else { return None };
    let v = a.result.as_ref()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    // A move retried once the base was already there reads no move: it moved from the action's base.
    if let ActionWhat::MoveBase { from, target, .. } = &a.what {
        if v.get("by_hand").is_some() {
            return Some((None, Some(target.to_string())));
        }
        match (s("before"), s("after")) {
            (Some(before), Some(after)) if before != after => Some((Some(before), Some(after))),
            _ => Some(((from != target).then(|| from.to_string()), Some(target.to_string()))),
        }
    } else {
        let before = v
            .get("parents")
            .and_then(Value::as_array)
            .filter(|p| p.len() > 1)
            .and_then(|p| p.first())
            .and_then(Value::as_str)
            .map(str::to_owned);
        Some((before, s("commit")))
    }
}

/// What a reload brought: its lines, and the cards among them landed with no span known, a commit
/// made by hand on the base one forked from counted as its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Brought {
    pub lines: Vec<String>,
    pub unbounded: Vec<String>,
}

/// Lists newly included cards once in landing order, then commits without a card oldest first.
/// `range` contains the commits newly in the target; landings outside it are excluded.
pub fn brought(range: &[Commit], landed: &[Landed]) -> Brought {
    let commits: BTreeMap<&str, &Commit> = range.iter().map(|c| (c.sha.as_str(), c)).collect();
    let landings: Vec<&Landed> = landed.iter().filter(|l| commits.contains_key(tip(l))).collect();
    let mut owned: BTreeSet<&str> = BTreeSet::new();
    for l in &landings {
        match (l.before.as_deref(), l.after.as_deref()) {
            (Some(before), Some(after)) => {
                let base = reach(&commits, before);
                owned.extend(reach(&commits, after).difference(&base).copied());
            }
            (None, Some(after)) => {
                owned.insert(after);
            }
            (_, None) => owned.extend(reach(&commits, &l.head)),
        }
    }
    let bare = range.iter().filter(|c| !owned.contains(c.sha.as_str()));
    let (mut cards, mut unbounded): (Vec<&Landed>, Vec<String>) = (Vec::new(), Vec::new());
    for l in landings {
        if cards.iter().all(|c| c.card != l.card) {
            cards.push(l);
        }
        if (l.before.is_none() || l.after.is_none()) && !unbounded.contains(&l.card) {
            unbounded.push(l.card.clone());
        }
    }
    let line = |c: &&Landed| if c.title.is_empty() { c.card.clone() } else { format!("{}: {}", c.card, c.title) };
    let mut lines: Vec<String> = cards.iter().map(line).collect();
    lines.extend(bare.map(|c| format!("{} {}", c.sha.chars().take(7).collect::<String>(), c.subject)));
    Brought { lines, unbounded }
}

fn tip(l: &Landed) -> &str {
    l.after.as_deref().unwrap_or(l.head.as_str())
}

// A commit outside the range is in the old build, and so is every commit it reaches.
fn reach<'a>(commits: &BTreeMap<&'a str, &'a Commit>, from: &str) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(sha) = stack.pop() {
        let Some((&sha, &commit)) = commits.get_key_value(sha).filter(|(k, _)| !seen.contains(**k)) else { continue };
        seen.insert(sha);
        stack.extend(commit.parents.iter().map(String::as_str));
    }
    seen
}

/// The reload's line in main: `said`, naming the two builds, then what the new one brought, or
/// why that couldn't be read; a list that may lack commits made by hand says so.
pub fn note(said: &str, brought: Result<Brought, String>) -> String {
    match brought {
        Ok(b) if b.lines.is_empty() => format!("{said} It brought nothing new."),
        Ok(b) if b.unbounded.is_empty() => format!("{said} It brought:\n- {}", b.lines.join("\n- ")),
        Ok(b) => format!(
            "{said} It brought:\n- {}\nPossibly incomplete: {} landed without a recorded base, so a commit made by \
             hand before such a card forked may be missing.",
            b.lines.join("\n- "),
            b.unbounded.join(", ")
        ),
        Err(why) => format!("{said} What it brought couldn't be read: {why}"),
    }
}
