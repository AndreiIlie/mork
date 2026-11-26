//! The Bitbucket token's identity (3.7.2): a refusal holds every Bitbucket action, and its recovery runs again only
//! what it held.

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::{AskKind, Color};
use serde_json::json;

use super::actions::{close_asks, hold};
use super::{Ctx, Decision, Intent};
use crate::messages::asks::{self, Raise};
use crate::messages::places::MAIN;
use crate::post::Poster;
use crate::state::{Change, State};

pub(super) fn token_failure(p: &mut Poster<'_>, a: Action, words: &str) {
    p.other.push(Change::SetTokenRefused(Some(words.to_owned())));
    p.journal("token", None, json!({"refused": words}));
    raise_token(p, words);
    hold(p, a, &format!("{}{words}", hold_prefix(p.token_forge)));
}

fn raise_token(p: &mut Poster<'_>, why: &str) {
    let text = format!("{}{why}. Fix the token file; the engine reads it again.", hold_prefix(p.token_forge));
    asks::raise(p, Raise::new(MAIN, AskKind::Token, Color::Red, text));
}

/// The token's identity, read (3.7.2): the operator's own, or refused, holds every Bitbucket action.
/// The verdict is stored, and changes only when a read differs from it.
pub fn token(state: &State, refused: Option<&str>, ctx: Ctx<'_>) -> Decision {
    if state.world.token_refused.as_deref() == refused {
        return Decision::default();
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.other.push(Change::SetTokenRefused(refused.map(str::to_owned)));
    p.journal("token", None, json!({"refused": refused}));
    match refused {
        Some(why) => raise_token(&mut p, why),
        None => {
            asks::close(&mut p, MAIN, &[AskKind::Token]);
            // Only what the token held runs again: an action held for another reason (a
            // merge Bitbucket refused, a PR declined) waits for its own answer (R1).
            let prefix = hold_prefix(p.token_forge);
            let held: Vec<Action> = state
                .actions
                .values()
                .filter(|a| a.status == ActionStatus::Held && is_forge(&a.what))
                .filter(|a| a.words.as_deref().is_some_and(|w| w.starts_with(&prefix)))
                .cloned()
                .collect();
            for mut a in held {
                a.status = ActionStatus::Pending;
                a.words = None;
                let id = a.id.clone();
                close_asks(&mut p, &id);
                p.put_action(a);
                p.intent(Intent::RunAction { id });
            }
        }
    }
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// How the words of an action the token held begin: what the token's recovery runs again.
pub(crate) fn hold_prefix(forge: &str) -> String {
    format!("the {forge} token can't act: ")
}

fn is_forge(w: &ActionWhat) -> bool {
    matches!(
        w,
        ActionWhat::Fetch { .. }
            | ActionWhat::Push { .. }
            | ActionWhat::OpenPr { .. }
            | ActionWhat::Comment { .. }
            | ActionWhat::ReplyComment { .. }
            | ActionWhat::ResolveComment { .. }
            | ActionWhat::Merge { .. }
    )
}
