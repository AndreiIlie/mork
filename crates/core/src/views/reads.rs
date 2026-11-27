use mork_types::CardId;
use mork_types::action::{Action, ActionStatus};
use mork_types::card::{Card, Stage};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::lifecycle::facts::{clear, mode_word};
use crate::state::State;

// A card's brief as it stands: the draft, waiting for its challenge until it is offered and then for approval,
// else the approved one, which says when its build started with nothing to clarify (fac-24, fac-597).
fn brief(run: Option<&Card>) -> Value {
    let Some(c) = run else { return Value::Null };
    let pending = c.stage == Stage::Brief && c.draft.is_some();
    let message = if pending { c.draft.as_ref() } else { c.brief.as_ref() };
    let Some(message) = message else { return Value::Null };
    let state = match (pending, c.recap, clear(c)) {
        (true, false, _) => "waiting for challenge",
        (true, true, _) => "waiting for approval",
        (false, _, true) => "nothing to clarify",
        (false, _, false) => "approved",
    };
    json!({"state": state, "message": message})
}

// The cards its run carries (fac-563), titled as their boards hold them.
fn carries(state: &State, run: Option<&mork_types::card::Card>) -> Vec<Value> {
    let all = run.map(|c| c.carries.as_slice()).unwrap_or_default();
    let title = |k: &mork_types::card::Carried| state.board.get(&k.id).map_or(k.title.clone(), |b| b.title.clone());
    all.iter().map(|k| json!({"card": k.id, "title": title(k)})).collect()
}

// `card.show <card>`: its board file's text and its run, each `null` when it has none.
pub(super) fn card(state: &State, card: &str) -> Decision {
    let id = CardId::new(card.to_lowercase());
    let on_board = state.board.get(&id);
    let run = state.cards.get(&id);
    let (title, project) = match (on_board, run) {
        (Some(b), _) => (b.title.clone(), b.project.clone()),
        (None, Some(c)) => (c.title.clone(), c.project.clone()),
        (None, None) => return Decision::refused(format!("no card `{card}` on a board or in the state")),
    };
    let file = on_board
        .and_then(|b| crate::board::read(&b.text, &crate::board::writes::written(state, &b.project, &b.file)).ok());
    let (description, notes, acceptance) = match (&file, run) {
        (Some(f), _) => (f.body.clone(), f.notes.clone(), f.criteria.clone()),
        (None, Some(c)) => {
            (c.description.clone(), Vec::new(), c.acceptance.iter().map(|a| (false, a.clone())).collect())
        }
        (None, None) => (String::new(), Vec::new(), Vec::new()),
    };
    let acceptance: Vec<Value> =
        acceptance.into_iter().map(|(done, text)| json!({"text": text, "done": done})).collect();
    let asks: Vec<Value> = state
        .asks
        .values()
        .filter(|a| a.open && a.card.as_ref() == Some(&id))
        .map(|a| json!({"id": a.id, "kind": crate::messages::asks::word(a.kind), "text": a.text, "place": a.place}))
        .collect();
    let run_json = run.map_or(Value::Null, |c| {
        json!({"run": c.run, "stage": c.stage.as_str(), "mode": mode_word(c.mode), "paused": c.paused,
               "head": c.head, "revisions": c.revisions, "closed": c.closed})
    });
    // A seat in its closing turn is ending: card.show lists the live ones (fac-417).
    let seats: Vec<Value> = super::place::seats(state, &id)
        .into_iter()
        .filter(|a| a.retiring.is_none())
        .map(|a| json!({"name": a.key, "role": a.role, "place": a.place}))
        .collect();
    Decision::ok(json!({"card.show": {
        "card": id.as_str(),
        "project": project,
        "title": title,
        "status": on_board.map(|b| b.status.clone()),
        "description": description,
        "notes": numbered(&notes),
        "acceptance": acceptance,
        "run": run_json,
        "carries": carries(state, run),
        "asks": asks,
        "brief": brief(run),
        "deliverables": deliverables(run),
        "seats": seats,
        "delegated": run.is_some_and(|c| c.delegated),
        "self_sustained": run.is_some_and(|c| c.delegated && c.self_sustained),
        "visual": run.is_some_and(|c| c.visual),
    }}))
}

// Each note with its number, the oldest's #1, which `card.change --withdraw` takes (fac-347).
fn numbered(notes: &[String]) -> Vec<Value> {
    let total = notes.len();
    let at = |i| mork_types::decisions::number(total, i);
    notes.iter().enumerate().map(|(i, text)| json!({"number": at(i), "text": text})).collect()
}

// Its deliverables in order (fac-128), each with the number the plan's commands take it by, its
// tasks, its current task's number (none before it has tasks), and whether it is worked on now.
fn deliverables(run: Option<&Card>) -> Value {
    let Some(plan) = run.and_then(|c| c.plan.as_ref()) else { return json!([]) };
    let one = |(i, d): (usize, &mork_types::card::Deliverable)| {
        let task = (!d.tasks.is_empty()).then_some(d.current + 1);
        json!({"n": i + 1, "name": d.name, "tasks": d.tasks, "task": task, "current": i == plan.current})
    };
    plan.deliverables.iter().enumerate().map(one).collect()
}

/// `projects`: each project's name, base, board prefix, where it lives, and its auto switch (fac-233).
pub(super) fn projects(state: &State, ctx: Ctx<'_>) -> Decision {
    let config = ctx.config;
    let bar = std::cell::OnceCell::new();
    let list: Vec<Value> = config
        .ordered_projects()
        .filter_map(|n| config.projects.get(n).map(|p| (n, p)))
        .map(|(name, p)| {
            let prefix = state.world.board_prefixes.get(name).cloned().or_else(|| {
                state.board.values().find(|b| &b.project == name && !b.prefix.is_empty()).map(|b| b.prefix.clone())
            });
            let auto = json!({"on": crate::messages::auto::on(state, name)});
            let live = crate::lifecycle::lookback::live(state, name);
            let passes = mork_types::lookback::Read::ALL.map(|read| {
                let config = p.passes.get(&read).cloned().unwrap_or_default();
                let last = state.lookbacks.get(name).and_then(|rows| rows.get(&read));
                let running = live.filter(|(_, active)| *active == read).map(|(id, _)| id);
                if config.mode == mork_types::lookback::Trigger::Auto {
                    use crate::lifecycle::trigger;
                    let base = trigger::baseline(state, name, read, ctx.now);
                    let bar = bar.get_or_init(|| trigger::starting_offer(state, ctx).as_ref().map(trigger::bar));
                    return json!({"read": read, "mode": config.mode, "last": last, "running": running,
                        "score": trigger::score(state, &base, ctx), "bar": bar});
                }
                json!({"read": read, "mode": config.mode, "landings": config.landings, "edits": config.edits,
                    "notes": config.notes, "days": config.days, "last": last, "running": running})
            });
            json!({"passes": passes, "name": name, "base": p.base, "prefix": prefix, "local": !p.forge.hosted(), "forge": p.forge.project_view(), "auto": auto})
        })
        .collect();
    Decision::ok(json!({"projects": {"projects": list}}))
}

/// `prs <project>`: a Bitbucket project's open PRs as the poller last read them (fac-28).
pub(super) fn prs(state: &State, project: &str, ctx: Ctx<'_>) -> Decision {
    let Some(cfg) = ctx.config.projects.get(project) else {
        return Decision::refused(format!("no project `{project}`"));
    };
    if !cfg.forge.hosted() {
        return Decision::refused(format!("{project}'s forge is local: it has no PRs"));
    }
    let Some(read) = state.world.open_prs.get(project) else {
        return Decision::refused(format!(
            "{project}'s open PRs aren't read yet: the poller reads them soon after a start"
        ));
    };
    let list: Vec<Value> = read
        .iter()
        .map(|p| {
            json!({"pr": p.pr, "title": p.title, "author": p.author, "source": p.branch, "destination": p.dest,
                   "updated": p.updated, "link": p.link})
        })
        .collect();
    Decision::ok(json!({"prs": {"project": project, "prs": list}}))
}

fn number(a: &Action) -> u64 {
    a.id.as_str().strip_prefix("act-").and_then(|n| n.parse().ok()).unwrap_or(u64::MAX)
}

/// `actions`: every action not done, oldest first; a held one with its words and its ask.
/// Dropped ones are only counted.
pub(super) fn actions(state: &State) -> Decision {
    let mut open: Vec<&Action> = state
        .actions
        .values()
        .filter(|a| matches!(a.status, ActionStatus::Pending | ActionStatus::Started | ActionStatus::Held))
        .collect();
    open.sort_by_key(|a| number(a));
    let list: Vec<Value> = open
        .into_iter()
        .map(|a| {
            let kind = serde_json::to_value(&a.what).ok().and_then(|v| v.get("kind").cloned());
            let ask = state.asks.values().find(|x| x.open && x.action.as_ref() == Some(&a.id)).map(|x| x.id.clone());
            json!({"id": a.id, "kind": kind, "status": a.status, "target": crate::decide::actions::words(&a.what),
                   "card": a.what.card(), "words": a.words, "ask": ask, "attempts": a.attempts,
                   "cancelled": a.cancelled})
        })
        .collect();
    let dropped = state.actions.values().filter(|a| a.status == ActionStatus::Dropped).count();
    Decision::ok(json!({"actions": {"actions": list, "dropped": dropped}}))
}
