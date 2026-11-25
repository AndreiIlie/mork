//! A board note's or a push's proof that nothing of it was written (fac-318, fac-394): a note only adds lines, so one
//! proven unwritten is made again from the file as it is now; a push's branch is renamed only while it holds.

use factory_types::action::{Action, ActionWhat, BoardEdit};
use factory_types::work::ActionFailure;

use crate::post::Poster;

/// A new action's proof: a standalone note and a push start unwritten, any other action has none.
pub fn proof(what: &ActionWhat) -> Option<bool> {
    let note = matches!(what, ActionWhat::ChangeBoardCard { edit: BoardEdit::Note { .. }, .. });
    (note || matches!(what, ActionWhat::Push { .. })).then_some(true)
}

/// A failure keeps the proof only when a check refused it before anything was replaced or pushed.
pub fn failed(a: &mut Action, f: &ActionFailure) {
    let checked = matches!(f, ActionFailure::Leak { .. } | ActionFailure::Internals { .. })
        && matches!(a.what, ActionWhat::Push { .. });
    if !checked && !matches!(f, ActionFailure::BoardUnwritten { .. }) {
        lose(a);
    }
}

/// An outcome that may have written something: the proof is gone for good, a push saved before it had one marked too.
pub fn lose(a: &mut Action) {
    if a.unwritten.is_some() || matches!(a.what, ActionWhat::Push { .. }) {
        a.unwritten = Some(false);
    }
}

/// A push given up without its proof, one saved before it had one: its doubt stays, as a lost proof's.
pub fn doubted(a: &mut Action) {
    if matches!(a.what, ActionWhat::Push { .. }) && a.unwritten.is_none() {
        a.unwritten = Some(false);
    }
}

/// An explicit retry of a note proven unwritten drops what it was made from, so the engine makes
/// it again from the file as it is; any other action keeps its checks.
pub fn refresh(p: &Poster<'_>, a: &mut Action) {
    if a.unwritten != Some(true) {
        return;
    }
    let ActionWhat::ChangeBoardCard { edit: BoardEdit::Note { .. }, before, after, text, dest, .. } = &mut a.what
    else {
        return;
    };
    for s in [before, after, text, dest] {
        s.clear();
    }
    a.result = None;
    let base = a.what.key();
    // The key stays unique in the state, as a new action's does.
    let taken = p.all_actions().any(|o| o.id != a.id && (o.key == base || o.key.starts_with(&format!("{base}@"))))
        || p.state.unloaded.key_taken(&base);
    a.key = if taken { format!("{base}@{}", a.id) } else { base };
}
