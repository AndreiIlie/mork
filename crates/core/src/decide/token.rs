//! The Bitbucket token's identity (3.7.2): a refusal holds every Bitbucket action, and its recovery runs again only
//! what it held.

use mork_types::action::{Action, ActionStatus};
use mork_types::ask::{AskKind, Color};
use serde_json::json;

use super::actions::{close_asks, hold};
use super::{Ctx, Decision, Intent};
use crate::messages::asks::{self, Raise};
use crate::messages::places::MAIN;
use crate::post::Poster;
use crate::state::{Change, State};

pub(super) fn token_failure(p: &mut Poster<'_>, a: Action, words: &str, ctx: Ctx<'_>) {
    let Some(project) = p.token_project(&a.what) else { return hold(p, a, words) };
    record(p, &project, Some(words), ctx);
    hold(p, a, &format!("{}{words}", hold_prefix(&project)));
}

fn record(p: &mut Poster<'_>, project: &mork_types::ProjectId, why: Option<&str>, ctx: Ctx<'_>) {
    if p.state.world.token_refused.get(project).map(String::as_str) == why { return }
    let prefix = hold_prefix(project);
    let open: Vec<_> = p.all_asks().filter(|a| a.open && a.kind == AskKind::Token && a.text.starts_with(&prefix))
        .map(|a| a.id.clone()).collect();
    for id in open { asks::close_one(p, &id); }
    if let Some(words) = why {
        let forge = ctx.config.forge_label(project);
        let text = format!("{prefix}{forge}: {words}. Fix the token file; the engine reads it again.");
        asks::raise(p, Raise::new(MAIN, AskKind::Token, Color::Red, text));
    }
    p.other.push(Change::SetTokenRefused(project.clone(), why.map(str::to_owned)));
    p.journal("token", None, json!({"project": project, "refused": why}));
}

pub fn token(state: &State, project: &mork_types::ProjectId, refused: Option<&str>, ctx: Ctx<'_>) -> Decision {
    if state.world.token_refused.get(project).map(String::as_str) == refused {
        return Decision::default();
    }
    let mut p = Poster::for_ctx(state, ctx);
    record(&mut p, project, refused, ctx);
    if refused.is_none() {
            let prefix = hold_prefix(project);
            let held: Vec<Action> = state
                .actions
                .values()
                .filter(|a| a.status == ActionStatus::Held && p.token_project(&a.what).as_ref() == Some(project))
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
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// How the words of an action the token held begin: what the token's recovery runs again.
pub(crate) fn hold_prefix(project: &str) -> String {
    format!("The token for project `{project}` cannot act: ")
}

