//! The operating material a session starts with, compared with the build that ran before it (fac-14): what changed is
//! announced to the agents holding the old: a manual as a diff, a role's to its role alone, the contract whole.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use factory_types::reply::Role;
use factory_types::state::Message;
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::announce::{self, post};
use crate::messages::places;
use crate::post::Poster;
use crate::state::State;

/// The operating material a start journaled: fingerprints, recoverable manual texts, and its build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub digests: BTreeMap<String, String>,
    pub build: Option<String>,
    pub texts: BTreeMap<String, String>,
}

impl Recorded {
    /// A baseline only when its text matches the fingerprint the previous start recorded.
    pub fn text(&self, key: &str) -> Option<&str> {
        let text = self.texts.get(key)?;
        (self.digests.get(key) == Some(&factory_types::hash::short(text))).then_some(text.as_str())
    }
}

/// Announces changed operating material, using verified `previous` texts for manual diffs or a notice without a baseline.
/// The reply contract stays whole; journals this build's manuals and digests for the next comparison.
pub(crate) fn material(state: &State, previous: Option<&Recorded>, build: &str, ctx: Ctx<'_>) -> Decision {
    let now = super::manuals::digests();
    let old = previous.map(|r| &r.digests);
    if old == Some(&now) && previous.and_then(|r| r.build.as_deref()) == Some(build) {
        return Decision::default();
    }
    let mut changed: Vec<&String> = now.keys().filter(|k| old.and_then(|p| p.get(*k)) != now.get(*k)).collect();
    // In the order a session's start gives them.
    changed.sort_by_key(|k| ["factory", "role:", "contract"].iter().position(|p| k.starts_with(p)));
    let texts = super::manuals::material();
    let mut p = Poster::for_ctx(state, ctx);
    for (place, roles) in listening(state) {
        for key in &changed {
            let role = key.strip_prefix("role:");
            if role.is_some_and(|r| !roles.iter().any(|x| x.as_str() == r)) {
                continue;
            }
            let (headline, text) =
                material_words(key, texts.get(*key).map_or("", String::as_str), previous.and_then(|r| r.text(key)));
            post(&mut p, &place, role, &headline, &text);
        }
    }
    let mut d = Decision::default();
    let manuals: BTreeMap<_, _> = texts.into_iter().filter(|(k, _)| k != "contract").collect();
    d.journal("material", None, json!({"digests": now, "build": build, "changed": changed, "texts": manuals}));
    p.finish(&mut d);
    d
}

/// Whether a stored message announces a manual's or the reply contract's change.
pub(crate) fn is_announcement(m: &Message) -> bool {
    announce::is(m)
        && (["factory", "contract"].into_iter().any(|key| m.headline == headline(key))
            || role_headline(&m.headline).is_some())
}

/// The role whose seats alone read a manual announcement: its addressee; none for any other announcement.
pub(crate) fn audience(m: &Message) -> Option<Role> {
    if !announce::is(m) {
        return None;
    }
    announce::addressed_role(m)
}

fn role_headline(text: &str) -> Option<Role> {
    Role::ALL.into_iter().filter(|r| *r != Role::Echo).find(|r| text == headline(r.as_str()))
}

// Places in use with an agent holding the old material: a started session, or a recorded prompt (a start in flight,
// a retry, a resume) its session will start on. A finished card's agents read nothing more.
fn listening(state: &State) -> BTreeMap<String, Vec<Role>> {
    let in_use = |place: &str| match places::card_of(state, place) {
        Some(c) => state.cards.get(&c).is_some_and(|c| c.stage.is_live()),
        None => true,
    };
    let holds = |a: &&factory_types::state::Agent| a.session_started || a.last_prompt.is_some();
    let mut out: BTreeMap<String, Vec<Role>> = BTreeMap::new();
    for a in state.agents.values().filter(holds).filter(|a| a.role != Role::Echo && in_use(&a.place)) {
        out.entry(a.place.to_string()).or_default().push(a.role);
    }
    out
}

fn headline(key: &str) -> String {
    let what = match key {
        "factory" => "The Factory manual".to_owned(),
        "contract" => "The reply contract".to_owned(),
        k => format!("The {} manual", k.strip_prefix("role:").unwrap_or(k)),
    };
    format!("{what} changed")
}

fn material_words(key: &str, text: &str, previous: Option<&str>) -> (String, String) {
    let headline = headline(key);
    let body = if key == "contract" {
        format!(
            "{headline} with the build the factory runs now. Its whole text follows and replaces the one this session was given.\n\n{}",
            text.trim()
        )
    } else {
        let name = key.strip_prefix("role:").unwrap_or(key);
        let notice = format!("{headline} with the build the factory runs now.");
        match previous {
            Some(old) => format!(
                "{notice} Removed lines start with `-`, added lines with `+`; surrounding lines give context.\n\n```diff\n{}```\n\nThe complete manual is available with `factory manual {name}`.",
                diff(old, text)
            ),
            None => format!(
                "{notice} The previous text could not be recovered. Read the complete manual with `factory manual {name}`."
            ),
        }
    };
    (headline, body)
}

/// A unified diff of two texts, its hunks with three lines of context.
pub(crate) fn diff(old: &str, new: &str) -> String {
    let lines = edits(old, new);
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, _) in lines.iter().enumerate().filter(|(_, (k, _))| *k != ' ') {
        let start = i.saturating_sub(3);
        let end = (i + 4).min(lines.len());
        if let Some(last) = ranges.last_mut().filter(|last| last.end >= start) {
            last.end = end;
        } else {
            ranges.push(start..end);
        }
    }
    let mut out = String::new();
    for range in ranges {
        let before = &lines[..range.start];
        let hunk = &lines[range];
        let old_count = hunk.iter().filter(|(k, _)| *k != '+').count();
        let new_count = hunk.iter().filter(|(k, _)| *k != '-').count();
        let old_start = before.iter().filter(|(k, _)| *k != '+').count() + usize::from(old_count > 0);
        let new_start = before.iter().filter(|(k, _)| *k != '-').count() + usize::from(new_count > 0);
        let _ = writeln!(out, "@@ -{old_start},{old_count} +{new_start},{new_count} @@");
        for (kind, text) in hunk {
            let _ = write!(out, "{kind}{text}");
            if !text.ends_with('\n') {
                out.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    out
}

fn edits<'a>(old: &'a str, new: &'a str) -> Vec<(char, &'a str)> {
    let a: Vec<_> = old.split_inclusive('\n').collect();
    let b: Vec<_> = new.split_inclusive('\n').collect();
    let mut out = Vec::with_capacity(a.len() + b.len());
    align(&a, &b, &mut out);
    out
}

// Hirschberg's split of a longest common subsequence: memory linear in the lines, however long the texts (fac-624).
fn align<'a>(a: &[&'a str], b: &[&'a str], out: &mut Vec<(char, &'a str)>) {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    out.extend(a[..head].iter().map(|l| (' ', *l)));
    let (a, b) = (&a[head..], &b[head..]);
    let tail = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (body_a, body_b) = (&a[..a.len() - tail], &b[..b.len() - tail]);
    match body_a {
        [] => out.extend(body_b.iter().map(|l| ('+', *l))),
        _ if body_b.is_empty() => out.extend(body_a.iter().map(|l| ('-', *l))),
        [line] => one(line, body_b, out),
        _ => {
            let mid = body_a.len() / 2;
            let split = split(&body_a[..mid], &body_a[mid..], body_b);
            align(&body_a[..mid], &body_b[..split], out);
            align(&body_a[mid..], &body_b[split..], out);
        }
    }
    out.extend(a[a.len() - tail..].iter().map(|l| (' ', *l)));
}

// One old line against the new ones: kept where it first recurs, else removed.
fn one<'a>(line: &'a str, b: &[&'a str], out: &mut Vec<(char, &'a str)>) {
    match b.iter().position(|l| *l == line) {
        Some(k) => {
            out.extend(b[..k].iter().map(|l| ('+', *l)));
            out.push((' ', line));
            out.extend(b[k + 1..].iter().map(|l| ('+', *l)));
        }
        None => {
            out.push(('-', line));
            out.extend(b.iter().map(|l| ('+', *l)));
        }
    }
}

// Where `b` splits so the two halves of `a` keep the longest common subsequence between them.
fn split(front: &[&str], back: &[&str], b: &[&str]) -> usize {
    let ahead = lengths(front.iter(), b.iter());
    let behind = lengths(back.iter().rev(), b.iter().rev());
    (0..=b.len()).rev().max_by_key(|&j| ahead[j] + behind[b.len() - j]).unwrap_or(0)
}

// The last row of the common-subsequence table: entry `j`, the longest common subsequence of `a` and `b`'s first `j`.
fn lengths<'x, 'y: 'x>(
    a: impl Iterator<Item = &'x &'y str>,
    b: impl Iterator<Item = &'x &'y str> + Clone,
) -> Vec<usize> {
    let n = b.clone().count();
    let mut row = vec![0; n + 1];
    let mut next = vec![0; n + 1];
    for x in a {
        for (j, y) in b.clone().enumerate() {
            next[j + 1] = if x == y { row[j] + 1 } else { row[j + 1].max(next[j]) };
        }
        std::mem::swap(&mut row, &mut next);
    }
    row
}
