//! Outward actions: returns, crash recovery, failures and held-action settlement (2.5, 2.5.7); the token's identity is
//! `token`'s.

use mork_types::action::{Action, ActionStatus, ActionWhat};
use mork_types::ask::{AskKind, Color};
use mork_types::reply::Role;
use mork_types::work::{ActionFailure, Found, Taken};
use mork_types::{ActionId, CardId};
use serde_json::{Value, json};

use super::token::token_failure;
use super::{Ctx, Decision, Intent};
use crate::door;
use crate::messages::asks::{self, Raise};
use crate::messages::places::MAIN;
use crate::messages::requests::ask_agent;
use crate::post::Poster;
use crate::state::{Change, State};

/// An action's words, for an ask.
pub fn words(w: &ActionWhat) -> String {
    match w {
        ActionWhat::Fetch { project, .. } => format!("the fetch of {project}"),
        ActionWhat::Take { card, head, .. } => format!("taking {card}'s head {}", head.short()),
        ActionWhat::TakeJoin { card, tip, .. } => format!("taking {card}'s join of {}", tip.short()),
        ActionWhat::Push { branch, commit, .. } => format!("the push of {branch} at {}", commit.short()),
        ActionWhat::BackupPush { project, base, remote, .. } => {
            format!("the backup push of {project}'s {base} to {remote}")
        }
        ActionWhat::OpenPr { branch, .. } => format!("opening the PR of {branch}"),
        ActionWhat::Comment { pr, file, .. } if file.is_empty() => format!("the comment on PR {pr}"),
        ActionWhat::Comment { pr, file, line, .. } => format!("the comment on PR {pr} at {file}:{line}"),
        ActionWhat::ReplyComment { pr, comment, .. } => format!("the reply to comment {comment} on PR {pr}"),
        ActionWhat::ResolveComment { pr, comment, .. } => format!("resolving comment {comment} on PR {pr}"),
        ActionWhat::Merge { pr, .. } => format!("the merge of PR {pr}"),
        ActionWhat::MoveBase { base, target, .. } => format!("moving {base} to {}", target.short()),
        ActionWhat::CreateBoardCard { title, .. } => format!("creating the board card `{title}`"),
        ActionWhat::ChangeBoardCard { file, .. } => format!("changing the board card {file}"),
        ActionWhat::RemoveClone { card, .. } | ActionWhat::RemoveTrain { card, .. } => {
            format!("removing {card}'s clone")
        }
        ActionWhat::Door(c) => format!("the door call {} on {} for {}", c.op, c.target, c.from.label()),
    }
}

fn place_of(state: &State, a: &Action) -> String {
    let ActionWhat::Door(c) = &a.what else { return a.what.card().map_or(MAIN.to_owned(), |c| c.as_str().to_owned()) };
    door::approval::ask_place(state, c).to_owned()
}

/// Holds an action with its words, and raises its red `held` ask (2.5.7).
pub fn hold(p: &mut Poster<'_>, mut a: Action, why: &str) {
    a.status = ActionStatus::Held;
    a.words = Some(why.to_owned());
    let place = place_of(p.state, &a);
    let mut r = Raise::new(&place, AskKind::Held, Color::Red, format!("{} is held: {why}", words(&a.what)));
    r.action = Some(a.id.clone());
    p.put_action(a);
    asks::raise(p, r);
}

/// A pending action, about to be carried out, is `started` (2.5.1); a cancelled or settled one
/// isn't, and the engine leaves it.
pub fn started(state: &State, id: &str, prepared: Option<&Value>, now: i64) -> Decision {
    let Some(mut a) = state.actions.get(id).cloned().filter(|a| a.status == ActionStatus::Pending && !a.cancelled)
    else {
        return Decision::default();
    };
    // A card's landing waits for its turns and jobs to end, a retry's and a restart's too (fac-10).
    if crate::queue::holds::landing_waits(state, &a.what) {
        return Decision::default();
    }
    let mut d = Decision::default();
    // A door call's guard runs here, so the operator's once is taken with the start, or not at all.
    if let Err(why) = crate::door::approval::start_guard(&mut a, prepared, now) {
        a.status = ActionStatus::Dropped;
        a.words = Some(why.clone());
        a.done_ms = Some(now);
        d.journal("action.dropped", a.what.card().map(|c| c.as_str()), json!({"id": a.id, "words": why}));
        d.changes.push(Change::PutAction(Box::new(a)));
        return d;
    }
    a.status = ActionStatus::Started;
    if a.result.is_none() && !matches!(a.what, ActionWhat::Door(_)) {
        a.result = prepared.cloned();
    }
    // Every move of an action is journaled (2.2.4, M3.33).
    d.journal("action.started", a.what.card().map(|c| c.as_str()), json!({"id": a.id, "key": a.key}));
    d.changes.push(Change::PutAction(Box::new(a)));
    d
}

pub fn returned(state: &State, id: &str, result: &Result<Value, ActionFailure>, ctx: Ctx<'_>) -> Decision {
    let Some(a) = state.actions.get(id).cloned() else { return Decision::default() };
    // A landing action whose start waited was never carried out: no result is its (fac-10).
    let unstarted = crate::queue::holds::landing_action(&a.what) && a.status != ActionStatus::Started;
    if unstarted || matches!(a.status, ActionStatus::Done | ActionStatus::Dropped) {
        return Decision::default();
    }
    let mut p = Poster::for_ctx(state, ctx);
    let removal = if let ActionWhat::RemoveClone { card, .. } = &a.what { Some(card.clone()) } else { None };
    match result {
        Ok(v) => done(&mut p, a, v.clone(), ctx),
        Err(_) if crate::lifecycle::retired::withdrawn(&p, &a) => crate::lifecycle::retired::drop_action(&mut p, a),
        Err(f) => failure(&mut p, a, f, ctx),
    }
    if let Some(card) = removal {
        crate::reviews::gate::reclone(&mut p, &card);
    }
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// An action done, and what follows from its result.
pub fn done(p: &mut Poster<'_>, mut a: Action, v: Value, ctx: Ctx<'_>) {
    let v = crate::queue::landing::merge_check(&a.what, v);
    // A merge whose check after the call failed stays held, with its alarm (R12).
    if let ActionWhat::Merge { card: c, .. } = &a.what
        && v.get("by_hand").is_none()
        && v.get("matches").and_then(Value::as_bool) == Some(false)
    {
        let why = v.get("why").and_then(Value::as_str).unwrap_or("the merge commit isn't what was tested");
        let words = format!("the merge of {c} needs a look: {why}");
        let (c, id) = (c.clone(), a.id.clone());
        a.status = ActionStatus::Held;
        a.result = Some(v.clone());
        a.words = Some(words.clone());
        p.put_action(a);
        if let Some(cd) = p.card(&c) {
            crate::queue::landing::alarm_on(p, cd, &words, Some(id));
        }
        return;
    }
    a.status = ActionStatus::Done;
    a.result = Some(v.clone());
    a.done_ms = Some(p.now);
    let (what, id) = (a.what.clone(), a.id.clone());
    p.journal("action.done", what.card().map(|c| c.as_str()), json!({"id": a.id, "key": a.key}));
    p.put_action(a);
    follows(p, &id, &what, &v, ctx);
}

fn follows(p: &mut Poster<'_>, id: &ActionId, what: &ActionWhat, v: &Value, ctx: Ctx<'_>) {
    let v = v.clone();
    let card = |p: &Poster<'_>, c: &CardId| p.card(c);
    match what {
        ActionWhat::Fetch { project, .. } => {
            crate::lifecycle::start::fetched(p, project, &v, ctx);
            crate::queue::train::fetched(p, project, &v, ctx);
            if let Some(tip) = v.get("base_tip").and_then(Value::as_str) {
                crate::queue::refresh::seen(p, project.as_str(), &mork_types::Sha::new(tip));
            }
        }
        ActionWhat::BackupPush { project, .. } => backed_up(p, project),
        ActionWhat::Take { card: c, head, job } => {
            if let (Some(cd), Ok(t)) = (card(p, c), serde_json::from_value::<Taken>(v.clone())) {
                crate::lifecycle::gates::take_returned(p, cd, head, job, t, ctx);
            }
        }
        ActionWhat::Push { card: c, commit, .. } => {
            if let Some(cd) = card(p, c) {
                crate::queue::pr::pushed(p, cd, commit, ctx.config);
            }
        }
        ActionWhat::OpenPr { card: c, .. } => {
            if let Some(cd) = card(p, c) {
                crate::queue::pr::pr_opened(p, cd, &v, ctx);
            }
        }
        ActionWhat::Merge { card: c, .. } => {
            if let Some(cd) = card(p, c) {
                crate::queue::landing::merged(p, cd, &v, ctx);
            }
        }
        ActionWhat::MoveBase { card: c, target, .. } => {
            if let Some(cd) = card(p, c) {
                crate::queue::landing::moved(p, cd, target, &v, ctx);
            }
        }
        ActionWhat::Comment { card: c, .. } => crate::reviews::findings::commented(p, c, id, &v, ctx),
        ActionWhat::ReplyComment { card, .. } => crate::reviews::findings::replied(p, card, id, ctx),
        // An older build's take of a join moves nothing now.
        ActionWhat::ResolveComment { .. } | ActionWhat::RemoveTrain { .. } | ActionWhat::TakeJoin { .. } => {}
        // A starting card waits for its status writes, its own card's and each it carries' (M3.23, fac-563).
        ActionWhat::ChangeBoardCard { card: Some(c), .. } => {
            if let Some(cd) = card(p, c) {
                crate::lifecycle::start::status_written(p, cd, id, ctx);
            }
        }
        // A door call's outcome reaches its requester by its answer, or by news when asked or held (fac-1, fac-632).
        ActionWhat::CreateBoardCard { .. } | ActionWhat::ChangeBoardCard { .. } | ActionWhat::Door(_) => {}
        ActionWhat::RemoveClone { card: c, run, .. } => {
            crate::memory::handed_off(p, c, *run, &v);
            crate::queue::pr_merged::removed(p, c, &v);
        }
    }
}

fn backed_up(p: &mut Poster<'_>, project: &str) {
    let held: Vec<Action> = p
        .all_actions()
        .filter(|a| a.status == ActionStatus::Held)
        .filter(|a| matches!(&a.what, ActionWhat::BackupPush { project: name, .. } if name.as_str() == project))
        .cloned()
        .collect();
    for mut a in held {
        close_asks(p, &a.id);
        a.status = ActionStatus::Dropped;
        a.words = Some("a later backup push succeeded".into());
        a.done_ms = Some(p.now);
        p.journal("action.dropped", None, json!({"id": a.id, "words": a.words}));
        p.put_action(a);
    }
}

fn failure(p: &mut Poster<'_>, mut a: Action, f: &ActionFailure, ctx: Ctx<'_>) {
    a.attempts += 1;
    crate::board::retry::failed(&mut a, f);
    match (f, &a.what) {
        (ActionFailure::Failed { words }, ActionWhat::Fetch { .. }) if a.attempts < 3 => retry_fetch(p, a, words),
        (ActionFailure::Internals { words }, ActionWhat::Push { .. }) => refused_push(p, a, words, ctx),
        (ActionFailure::Leak { words } | ActionFailure::Internals { words }, ActionWhat::ReplyComment { card, .. })
            if crate::reviews::responses::rejected(p, card, &a.id, words, ctx) => {}
        (
            ActionFailure::Leak { words } | ActionFailure::Internals { words },
            ActionWhat::Push { .. } | ActionWhat::Comment { .. },
        ) => sent_back(p, a, &words.clone(), ctx),
        (
            ActionFailure::Leak { words } | ActionFailure::Internals { words },
            ActionWhat::CreateBoardCard { .. } | ActionWhat::ChangeBoardCard { .. },
        )
        | (ActionFailure::Failed { words }, ActionWhat::CreateBoardCard { .. }) => {
            // Refused with nothing written (3.8.3, 4.18.5): whoever asked is told, and asks again.
            a.status = ActionStatus::Dropped;
            a.words = Some(words.clone());
            p.journal("action.refused", None, json!({"id": a.id, "words": words}));
            p.put_action(a);
        }
        (ActionFailure::Leak { words } | ActionFailure::Internals { words }, ActionWhat::OpenPr { .. }) => {
            let why = crate::queue::pr::held_words(words);
            hold(p, a, &why);
        }
        (ActionFailure::NotFastForward { at }, ActionWhat::Push { card, .. }) => {
            let c = card.clone();
            match p.card(&c) {
                // Moved from outside: the push is given up, and the card alarmed (4.11.6).
                Some(cd) if cd.pushed.as_ref() != at.as_ref() => {
                    a.status = ActionStatus::Dropped;
                    p.put_action(a);
                    crate::queue::landing::alarm(p, cd, "its branch on origin was moved from outside Mork");
                }
                // Refused with origin where Mork left it: nothing explains it, so it is
                // held for a look, never dropped silently (R4).
                _ => hold(p, a, "refused as not a fast-forward, with origin's branch where Mork left it"),
            }
        }
        (ActionFailure::Token { words }, _) => token_failure(p, a, words),
        // A read cut in flight isn't held: it fails, and asking again runs it anew.
        (ActionFailure::Gone { words }, _) if door::reads::read(&a) => door::reads::cut(p, a, words, None, ctx),
        (ActionFailure::Unstopped { words, session }, _) if door::reads::read(&a) => {
            door::reads::cut(p, a, words, Some(*session), ctx)
        }
        (ActionFailure::Conflict { .. }, ActionWhat::Merge { .. }) => crate::queue::approval::conflicted(p, a, f, ctx),
        (ActionFailure::Moved { words, tip }, _) => crate::queue::approval::moved(p, a, words, tip, ctx),
        // Held for the operator, never retried: a colleague's merge Bitbucket or its guard refused too (fac-95).
        (
            ActionFailure::Failed { words }
            | ActionFailure::BoardUnwritten { words }
            | ActionFailure::Gone { words }
            | ActionFailure::Unstopped { words, .. }
            | ActionFailure::Leak { words }
            | ActionFailure::Internals { words }
            | ActionFailure::Conflict { words, .. },
            _,
        )
        | (ActionFailure::Guard { words, .. }, ActionWhat::Merge { dest: Some(_), .. }) => hold(p, a, words),
        (ActionFailure::NotFastForward { .. }, _) => hold(p, a, "refused as not a fast-forward"),
        (ActionFailure::Guard { words, pr }, _) => guard_dropped(p, a, words, pr.as_deref(), ctx),
    }
}

fn retry_fetch(p: &mut Poster<'_>, mut a: Action, words: &str) {
    // Run again at its next need or poll (2.5.7); `done_ms` holds when it failed.
    a.status = ActionStatus::Pending;
    a.words = Some(words.to_owned());
    a.done_ms = Some(p.now);
    p.put_action(a);
}

// A guard no longer held right before the call drops the action, with why: no call is made (2.5.8).
fn guard_dropped(p: &mut Poster<'_>, mut a: Action, words: &str, pr: Option<&mork_types::work::PrSeen>, ctx: Ctx<'_>) {
    a.status = ActionStatus::Dropped;
    a.words = Some(words.to_owned());
    a.done_ms = Some(p.now);
    p.journal("action.dropped", a.what.card().map(|c| c.as_str()), json!({"id": a.id, "words": words}));
    let what = a.what.clone();
    p.put_action(a);
    if let (ActionWhat::Merge { card, .. }, Some(seen)) = (&what, pr)
        && let Some(c) = p.card(card)
    {
        crate::queue::pr::reread(p, c, seen, ctx);
    }
}

// A push refused for its branch's name goes on under a name the push accepts while it can (fac-394).
fn refused_push(p: &mut Poster<'_>, a: Action, words: &str, ctx: Ctx<'_>) {
    if let Some(a) = crate::queue::landing::repushed(p, a, words, ctx) {
        sent_back(p, a, words, ctx);
    }
}

// A hit of the leak or internals check in an agent's own words: they go back to it, and the
// action is dropped (2.5.7).
fn sent_back(p: &mut Poster<'_>, mut a: Action, words: &str, ctx: Ctx<'_>) {
    a.status = ActionStatus::Dropped;
    a.words = Some(words.to_owned());
    crate::board::retry::doubted(&mut a);
    let what = a.what.clone();
    p.put_action(a);
    if let ActionWhat::Push { card, .. } = &what
        && let Some(cd) = p.card(card)
    {
        let text = format!(
            "The push was refused: {words}. Take it out of the commits (their lines and their messages), then answer `built`."
        );
        crate::queue::members::send_back(p, cd, &text, ctx);
    }
    if let ActionWhat::Comment { card, .. } = &what {
        let text = format!("A comment was refused: {words}. Reword the finding, then post it again.");
        ask_agent(p, card, Role::Foreman, "Reword a comment", &text);
    }
}

/// What a crash found of a `started` action (2.5.2).
pub fn found(state: &State, id: &str, found: &Found, ctx: Ctx<'_>) -> Decision {
    let Some(mut a) = state.actions.get(id).cloned() else { return Decision::default() };
    let mut p = Poster::for_ctx(state, ctx);
    match found {
        Found::Done { result } => done(&mut p, a, result.clone(), ctx),
        Found::NotDone if crate::lifecycle::retired::withdrawn(&p, &a) => {
            crate::lifecycle::retired::drop_action(&mut p, a);
        }
        Found::NotDone => {
            crate::door::approval::not_dispatched(&mut a);
            a.status = ActionStatus::Pending;
            p.put_action(a);
            p.intent(Intent::RunAction { id: id.into() });
        }
        Found::Unclear { words } if door::reads::read(&a) => door::reads::cut(&mut p, a, words, None, ctx),
        Found::Unstopped { words, session } if door::reads::read(&a) => {
            door::reads::cut(&mut p, a, words, Some(*session), ctx)
        }
        Found::Unclear { words } | Found::Unstopped { words, .. } => {
            crate::board::retry::lose(&mut a);
            hold(&mut p, a, words);
        }
    }
    let mut d = Decision::default();
    p.finish(&mut d);
    d
}

/// The words of an action the operator dropped: an approved merge he dropped stands across polls (fac-98).
pub const DROPPED_BY_OPERATOR: &str = "dropped by the operator";

pub(super) fn close_asks(p: &mut Poster<'_>, id: &str) {
    let open: Vec<mork_types::AskId> =
        p.all_asks().filter(|x| x.open && x.action.as_deref() == Some(id)).map(|x| x.id.clone()).collect();
    for x in open {
        asks::close_one(p, &x);
    }
}

/// `action.retry`, `action.drop`, `action.done` (2.5.3).
pub fn settle_held(state: &State, id: &str, how: &str, note: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let Some(mut a) = state.actions.get(id).cloned() else { return Decision::refused(format!("no action `{id}`")) };
    if a.status != ActionStatus::Held {
        return Decision::refused(format!("{id} isn't held: it is {:?}", a.status).to_lowercase());
    }
    if let Some(why) = door::approval::refusal(&a, how) {
        return Decision::refused(why);
    }
    // Its submission was withdrawn: what it did is only marked done, or it is dropped (fac-591).
    if how == "retry" && a.withdrawn {
        return Decision::refused(format!("{id}'s submission was withdrawn: mark it done or drop it"));
    }
    let mut p = Poster::for_ctx(state, ctx);
    close_asks(&mut p, id);
    match how {
        "retry" => {
            crate::board::retry::refresh(&p, &mut a);
            crate::queue::pr::run(&mut p, a, ctx.config);
        }
        "drop" if matches!(a.what, ActionWhat::Door(_)) => {
            crate::door::approval::refuse(&mut p, a, DROPPED_BY_OPERATOR)
        }
        "drop" => {
            a.status = ActionStatus::Dropped;
            a.words = Some(DROPPED_BY_OPERATOR.to_owned());
            p.put_action(a);
        }
        _ => done(&mut p, a, json!({"by_hand": note.unwrap_or("")}), ctx),
    }
    let mut d = Decision::ok(json!({format!("action.{how}"): {"action": id}}));
    p.finish(&mut d);
    d
}
