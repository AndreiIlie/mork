//! A card's saved branch the push would refuse, rebuilt while nothing of it was published (fac-394): at the
//! engine's start, a run's start, and a push the checks refused.

use factory_types::action::{Action, ActionStatus};
use factory_types::card::{Card, Mode};
use serde_json::json;

use crate::decide::Ctx;
use crate::decide::actions::DROPPED_BY_ANDREI;
use crate::post::Poster;

/// The name a full card's saved branch is rebuilt to: the push's internals check refuses it, and no push of
/// it, in any run, is done or of an unknown outcome (started, held, its proof lost); none when it stays.
pub fn rebuilt(p: &Poster<'_>, c: &Card, ctx: Ctx<'_>) -> Option<String> {
    let words = &ctx.config.internals.words;
    let branch = c.branch.as_deref().filter(|_| c.mode == Mode::Full)?;
    crate::board::tickets::refused(words, branch)?;
    unpublished(p, c, branch).then(|| crate::board::tickets::branch(p.state, c, ctx.config)).filter(|b| b != branch)
}

/// Whether nothing of the card's work is published, nor on `branch` in any run: no push of it done or of an
/// unknown outcome.
pub fn unpublished(p: &Poster<'_>, c: &Card, branch: &str) -> bool {
    c.pushed.is_none() && c.pr.is_none() && c.carried_push.is_none() && !pushed(p, c, branch)
}

// Whether a push of the card to the branch, loaded or ended, may have published it: one an attempt of may have
// pushed, or one saved before the proof still under way, keeps its doubt; Andrei's own drop frees the name.
fn pushed(p: &Poster<'_>, c: &Card, branch: &str) -> bool {
    let prefix = format!("push:{}:{branch}:", c.id);
    let published = |s: ActionStatus| matches!(s, ActionStatus::Done | ActionStatus::Started | ActionStatus::Held);
    let by_andrei = |a: &Action| a.status == ActionStatus::Dropped && a.words.as_deref() == Some(DROPPED_BY_ANDREI);
    let unproven =
        |a: &Action| a.unwritten == Some(false) || (a.unwritten.is_none() && a.status != ActionStatus::Dropped);
    let doubted = |a: &Action| (published(a.status) || unproven(a)) && !by_andrei(a);
    let unloaded = p.state.unloaded.actions.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix));
    p.all_actions().any(|a| a.key.starts_with(&prefix) && doubted(a))
        || unloaded.into_iter().any(|(_, (_, s))| published(*s))
}

/// Rebuilds the card's saved branch when `rebuilt` gives one, and says whether it did.
pub fn rebuild(p: &mut Poster<'_>, c: &mut Card, ctx: Ctx<'_>) -> bool {
    let Some(name) = rebuilt(p, c, ctx) else { return false };
    p.journal("card.branch", Some(c.id.as_str()), json!({"from": c.branch, "to": name}));
    c.branch = Some(name);
    true
}

/// At the engine's start: every live card's (its review card found by its subject, fac-394).
pub fn recover(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let live: Vec<Card> = p.state.cards.keys().filter_map(|id| p.card(id)).filter(|c| c.stage.is_live()).collect();
    for mut c in live {
        if rebuild(p, &mut c, ctx) {
            p.put_card(c);
        }
    }
}
