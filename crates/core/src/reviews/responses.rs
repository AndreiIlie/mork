use factory_types::action::{ActionStatus, ActionWhat};
use factory_types::card::{Card, Mode};
use factory_types::review::Verdict;
use factory_types::state::{MessageKind, Sender};
use factory_types::{ActionId, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

pub(super) fn respond(
    state: &State,
    from: &Sender,
    card: &CardId,
    (n, text, dropped): (u64, &str, bool),
    ctx: Ctx<'_>,
) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(mut c) = state.cards.get(&id).cloned().filter(|c| c.mode == Mode::Review && c.review.is_some()) else {
        return Decision::refused(format!("{id} isn't a review card"));
    };
    if let Err(words) = super::findings::current(state, from, &c).and_then(|()| eligible(&c, n, text, dropped)) {
        return Decision::refused(words);
    }
    let Some(pr) = c.pr else { return Decision::refused("the finding has no PR") };
    let Some(commit) = c.head.clone() else { return Decision::refused("the review has no head") };
    let Some(r) = c.review.as_deref_mut() else { return Decision::refused("the card has no review") };
    let Some(f) = r.raised.iter_mut().find(|f| u64::from(f.n) == n) else {
        return Decision::refused("no such finding");
    };
    let Some(comment) = f.comment else { return Decision::refused("the finding has no comment") };
    let repo = crate::queue::pr::repo_of(&c, ctx.config);
    let mut p = Poster::for_ctx(state, ctx);
    let what = ActionWhat::ReplyComment { card: id.clone(), pr, comment, commit, body: text.trim().into(), repo };
    let action = p.act_as(what, true);
    remember(&mut c, n, &action, dropped);
    p.put_card(c);
    p.journal(
        if dropped { "review.resolved" } else { "review.answered" },
        Some(id.as_str()),
        json!({"n": n, "from": from.label(), "dropped": dropped, "action": action}),
    );
    let name = if dropped { "resolve" } else { "answer" };
    let mut d = Decision::ok(json!({name: {"card": id, "n": n, "action": action}}));
    p.finish(&mut d);
    d
}

fn eligible(c: &Card, n: u64, text: &str, dropped: bool) -> Result<(), String> {
    let f = c
        .review
        .as_ref()
        .and_then(|r| r.raised.iter().find(|f| u64::from(f.n) == n))
        .ok_or_else(|| format!("{} has no finding #{n}", c.id))?;
    if text.trim().is_empty() {
        return Err("a reply needs the reviewer's words".into());
    }
    if f.resolved {
        return Err(format!("finding #{n} is already resolved"));
    }
    if !dropped && f.answer.is_some() {
        return Err(format!("finding #{n} already has its one answer; later replies may still justify dropping it"));
    }
    if !f.thread.as_ref().is_some_and(|t| !t.replies.is_empty()) {
        return Err(format!("no person's reply was read on finding #{n}'s thread"));
    }
    Ok(())
}

fn remember(c: &mut Card, n: u64, action: &ActionId, dropped: bool) {
    let Some(r) = c.review.as_deref_mut() else { return };
    if let Some(f) = r.raised.iter_mut().find(|f| u64::from(f.n) == n) {
        if dropped {
            f.dropped = Some(action.clone());
            f.resolved = true;
        } else {
            f.answer = Some(action.clone());
        }
    }
    r.comments.push(action.clone());
}

pub(super) fn replied(p: &mut Poster<'_>, id: &CardId, action: &ActionId, ctx: Ctx<'_>) {
    let Some(c) = p.card(id) else { return };
    let dropped = c.review.as_ref().and_then(|r| r.raised.iter().find(|f| f.dropped.as_ref() == Some(action)));
    if let Some(f) = dropped {
        if f.thread.as_ref().is_none_or(|t| t.resolved.is_none())
            && let Some(comment) = f.comment
        {
            super::findings::thread(p, &c, comment, ctx);
        }
        let passed =
            c.review.as_ref().and_then(|r| r.rounds.last()).is_some_and(|r| r.verdict == Some(Verdict::Passed));
        if passed {
            super::end::release(p, id, ctx.config);
        }
    }
}

/// Returns rejected reviewer words without counting an answer or closing its finding (fac-448).
pub(crate) fn rejected(p: &mut Poster<'_>, id: &CardId, action: &ActionId, words: &str, ctx: Ctx<'_>) -> bool {
    let Some(mut c) = p.card(id) else { return false };
    let Some(r) = c.review.as_deref_mut() else { return false };
    let Some(f) = r.raised.iter_mut().find(|f| f.answer.as_ref() == Some(action) || f.dropped.as_ref() == Some(action))
    else {
        return false;
    };
    if f.answer.as_ref() == Some(action) {
        f.answer = None;
    } else {
        f.dropped = None;
        f.resolved = false;
    }
    let (by, n, underway) = (f.by.clone(), f.n, r.open().is_some());
    p.put_card(c);
    if let Some(mut a) = p.action(action.as_str()) {
        a.status = ActionStatus::Dropped;
        a.words = Some(words.to_owned());
        p.put_action(a);
    }
    let key = factory_types::AgentKey::new(format!("{id}/{by}"));
    if let Some(who) = crate::messages::places::who_of(p.state, &key) {
        let kind = if underway { MessageKind::Request } else { MessageKind::Update };
        let text = format!(
            "Finding #{n}'s reply was refused: {words}. Nothing was posted or counted. Reword it with the same command in a turn of the round under way; if that round ended, reword it when the next round asks you to verify this finding."
        );
        let seq = p.post(who.place.as_str(), Sender::Engine, Some(&by), kind, "Reword the thread reply", &text);
        if underway && !p.deliver(&who, &Sender::Engine, seq) {
            p.wake(&who, &Sender::Engine);
        }
    }
    super::results::done_once_posted(p, id, ctx);
    true
}
