//! Rejects commands whose sender lacks the required authority (fac-94, fac-95, fac-102).
//! All command entry points share this check so agents cannot bypass the operator's decisions.

use mork_types::Command;
use mork_types::action::ActionWhat;
use mork_types::memory::GENERAL;
use mork_types::protocol::{ErrorBody, ErrorCode};
use mork_types::reply::Role;
use mork_types::state::Sender;
use mork_types::{AgentKey, CardId};

use crate::decide::Ctx;
use crate::state::State;

/// A buzzed command's decision: its refusal, which the journal records (3.6.4, M3.20).
pub fn buzzed(from: &Sender, cmd: &Command, e: ErrorBody) -> crate::decide::Decision {
    let mut d = crate::decide::Decision::default();
    let card = if let Command::Merge { card } = cmd { Some(card.as_str().to_lowercase()) } else { None };
    d.journal("buzz", card.as_deref(), serde_json::json!({"command": cmd.name(), "from": from, "words": e.message}));
    d.answer = Some(Err(e));
    d
}

/// Whether a command is buzzed, and why; run on every command before its handler.
pub fn buzz(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Option<ErrorBody> {
    if matches!(cmd, Command::Config {} | Command::ConfigChange { .. }) && *from != Sender::Operator {
        return Some(ErrorBody::new(ErrorCode::Buzz, "Settings commands are the operator’s alone."));
    }
    if let Some(e) = door_buzz(state, from, cmd) {
        return Some(e);
    }
    // Every message an agent sends carries its kind, a `say` as a reply's (fac-18); the operator's needs none.
    if let (Command::Say { kind: None, .. }, Sender::Agent(_)) = (cmd, from) {
        return Some(ErrorBody::new(
            ErrorCode::Buzz,
            "An agent's message carries its kind, as a reply's does: `--kind request`, `response` or `update`.",
        ));
    }
    if let Command::MessageSendNow { place, seq } = cmd {
        let target = crate::messages::receipts::posted(state, place, *seq)?;
        return stopping(state, from, Some(&target.agent.key), true);
    }
    if let Some(e) = approve_buzz(state, from, cmd) {
        return Some(e);
    }
    if let (Command::ReviewMerge { project, pr }, false) = (cmd, *from == Sender::Operator) {
        return Some(ErrorBody::new(
            ErrorCode::Buzz,
            format!(
                "Reviewing and merging {project}'s PR {pr} is the operator's: nothing merges a colleague's PR on an agent's or an orchestrator's word. A plain `review` reports its findings to him."
            ),
        ));
    }
    if let Some(e) = other_buzz(state, from, cmd, ctx) {
        return Some(e);
    }
    // Both or neither of card and agent is the command's own shape refusal, not a question of authority.
    if let Command::Stop { card, agent } | Command::Resume { card, agent } = cmd
        && card.is_some() != agent.is_some()
    {
        return stopping_as(state, from, agent.as_ref(), false, &cmd.name());
    }
    if let Command::Resolve { card, n, .. } | Command::Answer { card, n, .. } = cmd {
        let declines = matches!(cmd, Command::Resolve { declined: Some(true), dropped: None, .. });
        return crate::reviews::findings::buzz(state, from, card, *n, declines);
    }
    // `action.retry` of a merge is a merge (M3.1): the same consent is needed.
    let card = if let Command::Merge { card } = cmd {
        card.clone()
    } else if let Command::ActionRetry { action } = cmd
        && let Some(ActionWhat::Merge { card, .. } | ActionWhat::MoveBase { card, .. }) =
            state.actions.get(action.as_str()).map(|a| &a.what)
    {
        card.clone()
    } else {
        return None;
    };
    let c = state.cards.get(&CardId::new(card.as_str().to_lowercase()))?;
    let project = ctx.config.projects.get(&c.project)?;
    merge_buzz(from, c, card.as_str(), project)
}

fn other_buzz(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Option<ErrorBody> {
    orchestrator_buzz(from, cmd)
        .or_else(|| triage_buzz(state, from, cmd))
        .or_else(|| memory_buzz(state, from, cmd, ctx))
        .or_else(|| claude_account_buzz(from, cmd))
        .or_else(|| alarm_buzz(state, from, cmd))
        .or_else(|| spawn_buzz(state, from, cmd))
        .or_else(|| preview_buzz(from, cmd))
        .or_else(|| run_buzz(from, cmd))
        .or_else(|| rereview_buzz(from, cmd))
        .or_else(|| rebrief_buzz(from, cmd))
        .or_else(|| findings_triage_buzz(state, from, cmd))
}

// A project's stream is its orchestrator's (fac-119, fac-244): nobody else hooks it; only the operator gives an ask,
// changes a started card's delegation level (fac-474), a project's auto, or sends a section to review (fac-230).
fn orchestrator_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let words = if let (Command::Hook {}, false) = (cmd, matches!(from, Sender::Orchestrator(_))) {
        "`hook` is a project's orchestrator's stream: it hooks with `mork ctl --as orchestrator:<project> hook`, nobody else.".to_owned()
    } else if let (Command::Wakes {}, false) = (cmd, matches!(from, Sender::Orchestrator(_))) {
        "`wakes` is a project's orchestrator's: use `mork ctl --as orchestrator:<project> wakes`.".to_owned()
    } else if let (Command::Give { ask }, false) = (cmd, *from == Sender::Operator) {
        format!("Giving {ask} to its orchestrator is the operator's: his button hands it an ask, nobody else does.")
    } else if let (Command::Opinion { ask }, false) = (cmd, *from == Sender::Operator) {
        format!(
            "Asking an orchestrator for an opinion on {ask} is the operator's: his button requests advice, nobody else does."
        )
    } else if let (Command::Delegate { card, .. }, false) = (cmd, *from == Sender::Operator) {
        format!(
            "Setting {card}'s delegation level is the operator's: his control in {card}'s room sets whether its asks and non-visual plans are given to its project's orchestrator. An orchestrator names a level only as it starts a card (`start --delegated` or `start --self-sustained`)."
        )
    } else if let (Command::Auto { project, .. }, false) = (cmd, *from == Sender::Operator) {
        format!("Turning {project}'s auto on or off is the operator's: his toggle does it, nobody else does.")
    } else if let (Command::Lookback { project, .. }, false) = (cmd, *from == Sender::Operator) {
        format!("Starting {project}'s look-back is the operator's: no seat or orchestrator starts one.")
    } else if let (Command::OrchestratorReview { .. }, false) = (cmd, *from == Sender::Operator) {
        "Sending a section to review is the operator's: his buttons in main's tabs send it, nobody else does."
            .to_owned()
    } else {
        return None;
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// A Dream seat's project is checked here and each note's kind before the deletion.
fn triage_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::NotesDelete { project, .. } = cmd else { return None };
    if *from == Sender::Operator
        || from.is_orchestrator()
        || crate::memory::dream_foreman(state, from).is_some_and(|c| c.project == *project)
    {
        return None;
    }
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        "Only the operator, an orchestrator or a Dream foreman folding its project's lessons may delete notes."
            .to_owned(),
    ))
}

fn memory_buzz(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Option<ErrorBody> {
    let (Command::MemoryWrite { memory, .. } | Command::MemoryDelete { memory, .. }) = cmd else { return None };
    let owned = |p: &mork_types::ProjectId| {
        if memory == GENERAL { ctx.config.mork.project.as_ref() == Some(p) } else { p.as_str() == memory }
    };
    let words = match from {
        Sender::Operator => return None,
        Sender::Orchestrator(p) if owned(p) => return None,
        Sender::Agent(_)
            if memory != GENERAL
                && crate::memory::dream_foreman(state, from).is_some_and(|c| c.project.as_str() == memory) =>
        {
            return None;
        }
        Sender::Agent(key)
            if state.agents.get(key).is_some_and(|a| a.retiring.as_ref().is_some_and(|r| r.asked && !r.resumed)) =>
        {
            let own = crate::memory::seat_card(state, key);
            if memory != GENERAL && own.is_some_and(|c| c.project.as_str() == memory) {
                return None;
            }
            format!(
                "On your closing turn you may edit only your card's project memory. {}",
                crate::memory::lesson_note_help(ctx.config.mork.project.as_ref())
            )
        }
        Sender::Agent(_) => format!(
            "Writing or deleting your project's memory requires your closing turn. {}",
            crate::memory::lesson_note_help(ctx.config.mork.project.as_ref())
        ),
        Sender::Engine | Sender::Orchestrator(_) | Sender::Pa if ctx.config.mork.project.is_some() => format!(
            "Writing or deleting the `{memory}` memory isn't yours: an orchestrator writes its own project's memory, only Mork's orchestrator writes `{GENERAL}`, and the operator writes any."
        ),
        Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => format!(
            "Writing or deleting the `{memory}` memory isn't yours: an orchestrator writes its own project's memory, and the operator writes any (`{GENERAL}` is the operator's alone when no Mork project is configured)."
        ),
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

fn claude_account_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    if !matches!(cmd, Command::ClaudeAccount { .. }) || *from == Sender::Operator || from.is_orchestrator() {
        return None;
    }
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        "Switching the shared Claude account is the operator's or an orchestrator's: every seat is refused.",
    ))
}

// Closing a card's alarm is the operator's, or an orchestrator's: its own foreman doesn't clear it (fac-157).
fn alarm_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::Ack { ask } = cmd else { return None };
    if *from == Sender::Operator || from.is_orchestrator() {
        return None;
    }
    state.asks.get(ask.as_str()).filter(|a| a.kind == mork_types::ask::AskKind::Alarm)?;
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        format!(
            "Closing the alarm {ask} is the operator's, or an orchestrator's: no agent clears a card's alarm, its foreman's neither."
        ),
    ))
}

fn spawn_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    use mork_types::reply::Role;
    let Command::Spawn { card, role, .. } = cmd else { return None };
    let id = CardId::new(card.as_str().to_lowercase());
    let owns = |r| matches!(from, Sender::Agent(k) if *k == crate::messages::requests::agent_of(&id, r).key);
    let full = state.cards.get(&id).is_some_and(|c| c.mode == mork_types::card::Mode::Full);
    if owns(Role::Foreman) || (full && *role == Role::Researcher && owns(Role::Planner)) {
        return None;
    }
    let words = format!(
        "{card}'s foreman spawns its researchers and reviewers where allowed, and its baseline seat at a visual \
         card's baseline step. Its planner spawns researchers."
    );
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// A card's preview runs on the operator's call (fac-222); its own foreman may only restart it while it is on (fac-348).
fn preview_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let (Command::Preview { card, .. } | Command::Demo { card }) = cmd else { return None };
    let foreman = format!("{}/foreman", card.as_str().to_lowercase());
    let restart = matches!(cmd, Command::Preview { action: None, .. })
        || matches!(cmd, Command::Preview { action: Some(a), .. } if a == "restart");
    if *from == Sender::Operator || (restart && matches!(from, Sender::Agent(k) if k.as_str() == foreman)) {
        return None;
    }
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        format!(
            "{card}'s preview is the operator's: he types /preview or /demo in {card}'s room. Its foreman may restart it while it is on (`preview {card} restart`); no seat starts or stops it."
        ),
    ))
}

// An orchestrator sets visual mode or delegation only at start; later changes remain the operator's (fac-504).
fn run_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let words = if let (Command::Visual { card, .. }, false) = (cmd, *from == Sender::Operator) {
        format!(
            "{card}'s visual mode is the operator's: his switch in {card}'s room turns it on or off; an orchestrator sets it only as it starts a card (`start --visual`). A foreman suggests it by raising its hand."
        )
    } else if let (Command::Mock { card }, false) = (cmd, *from == Sender::Operator) {
        format!(
            "Reopening {card}'s mock is the operator's alone: his `mock` among {card}'s actions. Suggest it by raising a hand."
        )
    } else if let (Command::Start { card, visual: Some(true), .. }, Sender::Agent(_)) = (cmd, from) {
        format!(
            "Starting {card} in visual mode is the operator's, or an orchestrator's: start it without `--visual` and suggest it by raising a hand."
        )
    } else if let (Command::Start { card, no_planner: Some(true), .. }, Sender::Agent(_)) = (cmd, from) {
        format!(
            "Starting {card} with no planner is the operator's, or an orchestrator's: start it without `--no-planner` and suggest it by raising a hand."
        )
    } else if let (Command::Start { card, delegated, self_sustained, .. }, Sender::Agent(_)) = (cmd, from)
        && (delegated.unwrap_or(false) || self_sustained.unwrap_or(false))
    {
        format!(
            "Starting {card} at a named delegation level is the operator's, or an orchestrator's: start it without `--delegated` or `--self-sustained` and suggest a level by raising a hand."
        )
    } else {
        return None;
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// Sending a card back to brief is the operator's or its own foreman's (fac-638): an orchestrator's neither.
fn rebrief_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::Brief { card, .. } = cmd else { return None };
    let card = card.as_str().to_lowercase();
    let own = matches!(from, Sender::Agent(a) if a.as_str() == format!("{card}/foreman"));
    if *from == Sender::Operator || own {
        return None;
    }
    let words = format!(
        "Sending {card} back to brief is the operator's or its own foreman's: a seat suggests it to {card}'s foreman, which raises it with the operator."
    );
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// Triaging failed review findings is the operator's, or the card's project's orchestrator's while given (fac-680).
fn findings_triage_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::Triage { card, .. } = cmd else { return None };
    if *from == Sender::Operator {
        return None;
    }
    let id = CardId::new(card.as_str().to_lowercase());
    let given = state.cards.get(&id).is_some_and(|c| {
        matches!(from, Sender::Orchestrator(p) if p == &c.project)
            && c.triage.as_ref().and_then(|t| state.asks.get(&t.ask)).is_some_and(|a| a.open && a.given)
    });
    if given {
        return None;
    }
    let words = format!(
        "Triaging {card}'s failed review findings is the operator's, or its project's orchestrator's while {card}'s open triage ask is given to it."
    );
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// Reviewing our card's PR again is its foreman's own judgement, the operator's or an orchestrator's (fac-349, fac-352).
fn rereview_buzz(from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::Rereview { card } = cmd else { return None };
    let id = CardId::new(card.as_str().to_lowercase());
    let foreman = crate::messages::requests::agent_of(&id, mork_types::reply::Role::Foreman).key;
    if *from == Sender::Operator || from.is_orchestrator() || from.label() == foreman.as_str() {
        return None;
    }
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        format!("Reviewing {id}'s PR again is its foreman's, the operator's or an orchestrator's: ask {foreman}."),
    ))
}

/// Whether `from` may stop or interrupt `target`'s turn (fac-78, fac-446): the operator anyone; an orchestrator
/// a whole card, or any agent only to interrupt; a foreman its spawned children; others no one.
pub fn stopping(state: &State, from: &Sender, target: Option<&AgentKey>, interrupt: bool) -> Option<ErrorBody> {
    stopping_as(state, from, target, interrupt, "stop")
}

fn stopping_as(
    state: &State,
    from: &Sender,
    target: Option<&AgentKey>,
    interrupt: bool,
    command: &str,
) -> Option<ErrorBody> {
    let verb = if command == "resume" { "Resuming" } else { "Stopping" };
    if from.is_orchestrator() {
        if interrupt || target.is_none() {
            return None;
        }
        let words = format!(
            "{verb} an agent is the operator's or the foreman's that spawned it: an orchestrator stops or resumes whole cards and may interrupt a turn with `say --delivery now`."
        );
        return Some(ErrorBody::new(ErrorCode::Buzz, words));
    }
    let Sender::Agent(me) = from else { return None };
    let words = match (target, target.and_then(|key| state.agents.get(key))) {
        (None, _) => format!(
            "{verb} a whole card is the operator's or an orchestrator's: no seat stops or resumes a card, its own included; a foreman stops or resumes only its own subagents, with `--agent`."
        ),
        // An unknown key is the command's own refusal to name: no foreman is wrong about an agent that isn't there.
        (Some(_), None) => return None,
        (Some(t), Some(agent)) if agent.parent.as_ref() == Some(me) => {
            if command != "resume" || agent.role != Role::Planner {
                return None;
            }
            format!("Resuming {t}'s turn is the operator's. Its foreman may stop or interrupt it.")
        }
        (Some(t), Some(_)) => format!(
            "{verb} {t}'s turn, or interrupting it, is the operator's, or the foreman's that spawned it (an orchestrator's too, to interrupt): {me} isn't its foreman."
        ),
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// Answering a door call's ask is the operator's alone, whichever command tries it.
fn door_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    if *from == Sender::Operator {
        return None;
    }
    let is_door = |id: &str| state.actions.get(id).is_some_and(|a| matches!(a.what, ActionWhat::Door(_)));
    let action = if let Command::DoorOnce { ask } | Command::DoorNo { ask } = cmd {
        state.asks.get(ask.as_str()).and_then(|a| a.action.clone()).unwrap_or_default()
    } else if let Command::ActionDrop { action } | Command::ActionDone { action, .. } = cmd
        && is_door(action.as_str())
    {
        action.clone()
    } else {
        return None;
    };
    let what = state
        .actions
        .get(action.as_str())
        .map_or_else(|| "a door call".to_owned(), |a| crate::decide::actions::words(&a.what));
    Some(ErrorBody::new(
        ErrorCode::Buzz,
        format!(
            "Answering {what} is the operator's: the door's asks are his alone. He answers with /door.once or /door.no on its ask."
        ),
    ))
}

// Landing is the merge train's: a card enters it when its gates pass and lands once its candidate
// passes, no agent's word moving it (fac-68, decision 3); `/merge` stays the operator's, as it was.
fn merge_buzz(
    from: &Sender,
    _c: &mork_types::card::Card,
    card: &str,
    project: &mork_types::config::ProjectConfig,
) -> Option<ErrorBody> {
    if *from == Sender::Operator {
        return None;
    }
    let base = &project.base;
    let forge = project.forge.label();
    let words = match project.forge.hosted() {
        false => format!(
            "Merging {card} into {base} is the merge train's: once its gates pass its candidate is verified, its review round and its suite, and it lands from the merge queue's front; nothing lands on {base} on an agent's or an orchestrator's word. The operator's /merge in {card}'s room says where it stands."
        ),
        true if !project.forge.merges_on_approval() => {
            format!("The operator merges {card} into {base} by hand on {forge}.")
        }
        true => format!(
            "Only the operator's approval on {forge} merges {card} into {base}: its PR is published once its gates pass and its candidate is verified, and his approval merges it while that verification stands on the approved head; nothing merges on an agent's or an orchestrator's word. The operator's /merge in {card}'s room says where it stands."
        ),
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

// Delegation belongs to the open plan ask, never to a mock, a visual card's plan (fac-636) or another project's
// orchestrator (fac-420).
fn approve_buzz(state: &State, from: &Sender, cmd: &Command) -> Option<ErrorBody> {
    let Command::Approve { card } = cmd else { return None };
    if *from == Sender::Operator {
        return None;
    }
    let id = CardId::new(card.as_str().to_lowercase());
    let c = state.cards.get(&id);
    let visual = c.is_some_and(|c| c.visual && c.stage == mork_types::card::Stage::Brief);
    if c.is_some_and(|c| !crate::lifecycle::mock::open(c) && matches!(from, Sender::Orchestrator(p) if p == &c.project))
        && !visual
        && state.asks.values().any(|a| crate::lifecycle::brief::is_given_plan(a, &id))
    {
        return None;
    }
    let words = if visual {
        format!(
            "{card} is a visual card: its plan, its mock included, is approved by the operator alone, whatever its delegation. Neither an agent nor an orchestrator approves it, and its approve ask is never given. He approves it with /approve or the approve button; words in {card}'s room don't."
        )
    } else if c.is_some_and(crate::lifecycle::mock::open) {
        format!(
            "Approving {card}'s mock is the operator's: neither an agent nor an orchestrator approves it for him. He approves it with /approve or the approve button; words in {card}'s room don't."
        )
    } else {
        format!(
            "The /approve command is the operator's, or its project's orchestrator's while {card}'s open approve-brief ask is given to it (given automatically on a delegated non-visual card; a delegated plan never approves itself). The operator may still approve a given plan himself. Use /approve or its button; words in the room never approve."
        )
    };
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}
