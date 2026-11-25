//! A local `/merge` in Andrei's checkout (4.11.3): every refusal and choice is made here (1.12.1, M3.35); the engine
//! rereads the facts right before it moves anything and acts at once on this answer.

use serde::{Deserialize, Serialize};

/// What the engine read of Andrei's checkout, right before the move.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    /// The branch checked out (`HEAD`), empty when detached.
    pub on: String,
    pub base: String,
    /// The base's commit now, and the commit the merge moves it to.
    pub cur: String,
    pub target: String,
    /// `target` is an ancestor of `cur`: the base is already past it.
    pub target_in_cur: bool,
    /// `cur` is an ancestor of `target`: the move is a fast-forward.
    pub cur_in_target: bool,
    /// The paths the move writes (`cur..target`).
    pub touched: Vec<String>,
    /// Tracked paths with local changes.
    pub changed: Vec<String>,
    /// Untracked, and ignored, files.
    pub loose: Vec<String>,
    pub ignored: Vec<String>,
}

/// What the engine does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    /// The base is at the target already.
    Done,
    /// Fast-forward the base to the target, now.
    FastForward,
    /// The base moved on from the fork point: a join is tested first.
    BaseMoved,
    /// Nothing moves, and why.
    Refused(String),
}

fn collides(a: &str, b: &str) -> bool {
    let inside = |outer: &str, inner: &str| inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('/'));
    a == b || inside(a, b) || inside(b, a)
}

/// The move, from the facts (4.11.3).
pub fn decide(f: &Facts) -> Move {
    if f.cur == f.target {
        return Move::Done;
    }
    if f.target_in_cur {
        return Move::Refused(format!("{} is already past {}: nothing to merge", f.base, short(&f.target)));
    }
    if !f.cur_in_target {
        return Move::BaseMoved;
    }
    if f.on != f.base {
        return Move::Refused(format!("Andrei's checkout is on `{}`, not on {}", f.on, f.base));
    }
    let touched = |p: &str| f.touched.iter().find(|t| collides(t, p));
    if let Some(p) = f.changed.iter().find(|p| touched(p).is_some()) {
        return Move::Refused(format!("{p} has local changes in Andrei's checkout, and the merge changes it"));
    }
    for p in f.loose.iter().chain(&f.ignored) {
        if let Some(t) = touched(p) {
            return Move::Refused(format!(
                "{p}, untracked or ignored in Andrei's checkout, stands where the merge writes {t}"
            ));
        }
    }
    Move::FastForward
}

fn short(s: &str) -> &str {
    s.get(..12).unwrap_or(s)
}
