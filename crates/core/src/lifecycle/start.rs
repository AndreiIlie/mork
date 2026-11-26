//! Starting a card (4.3): refused with nothing made, or recorded `starting` first; then the fetch, the clone and
//! `install`, the board's status, and the mode's first turn.

use mork_types::action::ActionStatus;
use mork_types::card::{BoardCard, Card, Mode, Stage};
use mork_types::config::{SeatPhase, Standing};
use mork_types::reply::Role;
use mork_types::state::{Agent, Choice, Sender};
use mork_types::work::Purpose;
use mork_types::{ActionId, CardId};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision, Intent};
use crate::messages::requests::ask_agent;
use crate::post::Poster;
use crate::state::State;

// A mode word (4.3.1).
fn mode(asked: Option<Mode>) -> Result<Mode, String> {
    match asked.unwrap_or(Mode::Full) {
        m @ (Mode::Full | Mode::Report) => Ok(m),
        Mode::Light => Err("`light` is no longer a mode: start it full".into()),
        Mode::Review => Err("`review` isn't a mode a card starts in: full or report".into()),
    }
}

/// A card that may run, alone or carried (4.3, fac-563): on a configured board, not live, and not done there.
pub(super) fn runnable<'s>(p: &Poster<'s>, id: &CardId, ctx: Ctx<'_>) -> Result<&'s BoardCard, String> {
    let Some(bc) = p.state.board.get(id) else { return Err(format!("no configured board holds {id}")) };
    if let Some(c) = p.card(id).filter(|c| c.stage.is_live()) {
        return Err(format!("{id} is live, at {}", c.stage.as_str()));
    }
    let project = ctx.config.projects.get(&bc.project).ok_or_else(|| format!("no project `{}`", bc.project))?;
    if bc.status == bc.done_status || bc.status == project.statuses.merged {
        return Err(format!("{id} is done on its board ({})", bc.status));
    }
    Ok(bc)
}

// Why a card can't start now, or `Ok`.
fn startable(p: &Poster<'_>, id: &CardId, ctx: Ctx<'_>) -> Result<(), String> {
    runnable(p, id, ctx)?;
    // A closed run's last bundle must keep its commits before a new run replaces the clone (fac-537).
    if crate::agents::closing::clone_going(p.state, id.as_str()) {
        return Err(format!("{id}'s last head is still being kept; start it again once its clone is removed"));
    }
    if crate::agents::closing::removal_held(p.state, id.as_str()) {
        return Err(format!("{id}'s clone removal is held: retry it, which keeps its mock aside, before a new run"));
    }
    super::carried::taken(p.state, id, None).map_or(Ok(()), Err)
}

// One port per live card (4.3.4).
pub(super) fn port(p: &Poster<'_>, ctx: Ctx<'_>) -> Option<u16> {
    let (from, to) = ctx.config.mork.port_range.split_once('-')?;
    let (from, to): (u16, u16) = (from.trim().parse().ok()?, to.trim().parse().ok()?);
    let live = p.state.cards.values().filter(|c| c.stage.is_live());
    let mut used: std::collections::BTreeSet<_> =
        live.flat_map(|c| [c.port, c.mock.and_then(|m| m.port)]).flatten().collect();
    used.extend(ctx.config.mork.web_port);
    used.extend(ctx.config.tunnels.values().map(|t| t.port));
    (from..=to).find(|n| !used.contains(n))
}

// Refusal must precede the fetch, clone and first turn of a new run.
fn start_in(
    p: &mut Poster<'_>,
    id: &CardId,
    mode: Mode,
    options: Options,
    agent: Option<Agent>,
    ctx: Ctx<'_>,
) -> Result<(), String> {
    startable(p, id, ctx)?;
    let Some(bc) = p.state.board.get(id).cloned() else { return Err(format!("no configured board holds {id}")) };
    let prior = p.card(id);
    let carries = super::carried::members(p, id, mode, &options.carries, prior.as_ref(), ctx)?;
    let run = prior.as_ref().map_or(1, |c| c.run + 1);
    let mut c = Card::new(id.clone(), &bc.project, &bc.title, mode, run, p.now);
    c.no_planner = options.no_planner;
    c.description = bc.description.clone();
    c.carries = carries;
    if let Some(rid) = reviewed(p, id, prior.as_ref(), mode, &c, ctx) {
        return Err(format!("{rid}, the operator's review of {id}'s branch, is under way: `done {rid}` first"));
    }
    let visual = super::visual::of_run(&c, options.visual, prior.as_ref())?;
    super::stops::stop_owed(p, id, ctx);
    start_seats(p, &c, agent);
    c.acceptance = bc.acceptance.clone();
    c.port = port(p, ctx);
    if let Some(prior) = prior.as_ref().filter(|x| x.mode != Mode::Review && mode != Mode::Review) {
        continued(&mut c, prior);
        // A branch an earlier run published keeps its name, whatever the push now refuses (fac-394).
        if prior.pushed.is_none() && prior.pr.is_none() {
            crate::lifecycle::branches::rebuild(p, &mut c, ctx);
        }
    }
    if options.delegated || options.self_sustained {
        c.delegated = true;
        c.self_sustained = options.self_sustained;
    } else {
        // Delegated until the operator turns it off, a new run too (fac-172); else by its project's auto (fac-381).
        c.delegated = prior.as_ref().is_some_and(|c| c.delegated) || crate::messages::auto::on(p.state, &c.project);
        c.self_sustained = prior.as_ref().is_some_and(|c| c.delegated && c.self_sustained);
    }
    c.visual = visual;
    c.first_seq = p.state.next_seq;
    // Merged since the last list, a carried PR would land the run: it is read at once (fac-171).
    if c.pr_unconfirmed {
        p.intent(Intent::ReadPr { card: id.clone() });
    }
    if crate::messages::places::two_rooms(&c) {
        p.touch_place(&crate::messages::places::spec_room(id));
    }
    p.put_card(c);
    p.touch_place(id.as_str());
    p.journal("card.start", Some(id.as_str()), json!({"run": run, "mode": crate::lifecycle::facts::mode_word(mode)}));
    p.fetch(&bc.project);
    Ok(())
}

// The first seats may already have a selection, but their conversations start at this run: a full card's foreman and
// its planner, the foreman's unnumbered child (fac-633), or a no-planner card's foreman alone (fac-616).
fn start_seats(p: &mut Poster<'_>, c: &Card, agent: Option<Agent>) {
    let mut seats: Vec<Agent> = Vec::new();
    let seat = |role: Role| {
        let made = || crate::post::new_agent(&crate::messages::requests::agent_of(&c.id, role));
        agent.clone().filter(|a| a.role == role).unwrap_or_else(made)
    };
    if c.mode == Mode::Full {
        let foreman = seat(Role::Foreman);
        if !c.no_planner {
            let mut planner = seat(Role::Planner);
            planner.parent = Some(foreman.key.clone());
            seats.push(planner);
        }
        seats.push(foreman);
    } else {
        seats.extend(agent.clone());
    }
    for key in p.state.agents.keys().filter(|k| k.as_str().split('/').next() == Some(c.id.as_str())) {
        if !seats.iter().any(|a| a.key == *key) {
            p.remove_agent(key);
        }
    }
    for mut a in seats {
        a.seen = p.next_seq;
        a.seen_before = a.seen;
        p.put_agent(a);
    }
}

// The live review of the branch a full card would take, which its gate never replaces.
fn reviewed(p: &Poster<'_>, id: &CardId, prior: Option<&Card>, mode: Mode, c: &Card, ctx: Ctx<'_>) -> Option<CardId> {
    let branch = prior.filter(|x| x.mode != Mode::Review).and_then(|x| x.branch.clone());
    let branch = branch.unwrap_or_else(|| crate::board::tickets::branch(p.state, c, ctx.config));
    let rid = crate::reviews::cards::branch_id(c.project.as_str(), &branch);
    (mode == Mode::Full).then(|| crate::reviews::gate::taken(p, &rid, id)).flatten()
}

// A later run goes on with the card's branch (fac-111): at its tip, its candidate pushed on top of
// the one published, into the same PR. A landed run's work is in the base: it starts there.
fn continued(c: &mut Card, prior: &Card) {
    c.branch = prior.branch.clone();
    if prior.stage != Stage::Done {
        c.pushed = prior.pushed.clone();
        c.previous_push = prior.previous_push.clone();
        c.pushed_head = prior.pushed_head.clone();
        c.verified_pushes = prior.verified_pushes.clone();
        // Its PR is the run's once read open (fac-171): the poll's list can be older than its merge.
        c.pr = prior.pr;
        c.pr_unconfirmed = prior.pr.is_some();
        c.carried_push = prior.pr.and(prior.pushed.clone());
        c.continues = prior.head.clone().or_else(|| prior.continues.clone());
    }
}

/// The first seat's model and effort a `start` names, either or both.
pub type Selection<'a> = (Option<&'a str>, Option<&'a str>);

/// A named delegation level overrides the prior run and project auto; `no_planner` is this run's alone, its foreman
/// holding the specification; `carries` are the cards the run takes with it.
pub struct Options {
    pub visual: bool,
    pub delegated: bool,
    pub self_sustained: bool,
    pub no_planner: bool,
    pub carries: Vec<CardId>,
}

/// Starts a run with optional mode, visual mode, delegation level and first seat selection.
pub fn start(
    state: &State,
    from: &Sender,
    card: &str,
    asked: Option<Mode>,
    options: Options,
    selection: Selection<'_>,
    ctx: Ctx<'_>,
) -> Decision {
    if options.delegated && options.self_sustained {
        return Decision::refused("choose either --delegated or --self-sustained");
    }
    let m = match mode(asked) {
        Ok(m) => m,
        Err(e) => return Decision::refused(e),
    };
    if options.no_planner && m != Mode::Full {
        let word = super::facts::mode_word(m);
        return Decision::refused(format!("`--no-planner` is a full card's: a {word} card has no planner"));
    }
    let id = CardId::new(card);
    let made = match picked(from, &id, (m, options.no_planner), selection, ctx) {
        Ok(a) => a,
        Err(e) => return Decision::refused(e),
    };
    let mut p = Poster::for_ctx(state, ctx);
    if let Err(e) = start_in(&mut p, &id, m, options, made, ctx) {
        return Decision::refused(e);
    }
    let mut d = Decision::ok(json!({"start": {"card": card}}));
    p.finish(&mut d);
    d
}

// Made now with its selection pending, its first turn launches with it as a `pick` would.
fn picked(
    from: &Sender,
    id: &CardId,
    (m, no_planner): (Mode, bool),
    selection: Selection<'_>,
    ctx: Ctx<'_>,
) -> Result<Option<Agent>, String> {
    let (model, effort) = selection;
    if model.is_none() && effort.is_none() {
        return Ok(None);
    }
    let role = if m == Mode::Full && !no_planner { Role::Planner } else { Role::Foreman };
    let phase = if m == Mode::Full { SeatPhase::Work } else { SeatPhase::Report };
    let mut a = crate::post::new_agent(&crate::messages::requests::agent_of(id, role));
    // The first turn uses its mode's phase, so a start-time pick isn't discarded at launch (fac-399, fac-402).
    let at = Standing { phase, review: None };
    let s =
        crate::agents::models::selection(ctx.config, &a, at, model, effort, None, crate::agents::models::By::Other)?;
    let by = if from.is_orchestrator() { from.label() } else { "operator".to_owned() };
    a.pending = Some(Choice { cli: s.model.cli, model: s.model.model, effort: s.effort, by, phase: Some(phase) });
    Ok(Some(a))
}

/// A fetch returned: the cards starting on its project get their base tip, their branch and
/// their clone; a colleague's review card its own (fac-95).
pub fn fetched(p: &mut Poster<'_>, project: &str, result: &Value, ctx: Ctx<'_>) {
    let Some(tip) = result.get("base_tip").and_then(Value::as_str).map(mork_types::Sha::new) else { return };
    let starting: Vec<Card> = p
        .state
        .cards
        .values()
        .map(|c| p.card(&c.id).unwrap_or(c.clone()))
        .chain(p.all_new_cards())
        .filter(|c| c.project == project && c.stage == Stage::Starting && c.base_tip.is_none())
        .collect();
    for mut c in starting {
        if c.mode == Mode::Review {
            crate::reviews::cards::fetched(p, c, &tip, result, ctx);
            continue;
        }
        // One branch per card, whatever its run: a later run goes on with it (fac-111).
        let branch = c.branch.clone().unwrap_or_else(|| crate::board::tickets::branch(p.state, &c, ctx.config));
        let at = continued_at(&c, &branch, result).unwrap_or_else(|| tip.clone());
        c.base_tip = Some(tip.clone());
        c.branch = Some(branch.clone());
        super::lookback::fetched(&mut c, &at);
        let id = c.id.clone();
        p.put_card(c);
        p.intent(Intent::StartJob { purpose: Purpose::Clone { card: id, at, branch } });
    }
}

// Where a later run's clone starts (fac-111): a head its run took and never published goes on;
// else its branch's tip as this fetch read it, a publication's merge or a newer push in it.
fn continued_at(c: &Card, branch: &str, result: &Value) -> Option<mork_types::Sha> {
    if c.continues.is_none() && c.pushed.is_none() {
        return None;
    }
    let unpublished = c.continues.clone().filter(|h| c.pushed_head.as_ref() != Some(h));
    let tip = result.get("tips").and_then(|t| t.get(branch)).and_then(Value::as_str).map(mork_types::Sha::new);
    unpublished.or(tip).or_else(|| c.continues.clone())
}

/// The clone is made: `install`, or the card's run goes on.
pub fn cloned(p: &mut Poster<'_>, mut card: Card, v: &Value, ctx: Ctx<'_>) {
    if card.stage != Stage::Starting {
        return;
    }
    super::lookback::cloned(&mut card, v);
    // The kept copy goes only once its put-back is recorded (fac-591).
    if let Some(m) = v.get("mock") {
        p.journal("mock.restored", Some(card.id.as_str()), m.clone());
        p.intent(Intent::DropMock { card: card.id.clone() });
    }
    if let Some(fork) = v.get("fork").and_then(Value::as_str) {
        card.fork = Some(mork_types::Sha::new(fork));
        p.put_card(card.clone());
    }
    let install = ctx.config.projects.get(&card.project).and_then(|pr| pr.install.clone());
    if install.is_some() {
        p.intent(Intent::StartJob { purpose: Purpose::Install { card: card.id.clone(), cloned: None } });
    } else {
        made(p, card, ctx);
    }
}

/// Everything made: the board's status, its own card's and each it carries, then the mode's first turn once this
/// run's writes are done (4.3.3, M3.23). A card with no board file goes straight on.
pub fn made(p: &mut Poster<'_>, mut card: Card, ctx: Ctx<'_>) {
    if card.stage != Stage::Starting {
        return;
    }
    if let Some(project) = ctx.config.projects.get(&card.project) {
        let of: Vec<CardId> = super::carried::cards(&card).cloned().collect();
        let started = &project.statuses.started;
        let ids: Vec<ActionId> =
            of.iter().filter_map(|k| super::carried::status_write(p, &card, k, started, false)).collect();
        if !written(p, &ids) {
            card.awaits = Some(ids);
            p.put_card(card);
            return;
        }
    }
    first_turn(p, card, ctx);
}

// A dropped write is never done: the run waits, whatever its other writes do.
fn written(p: &Poster<'_>, ids: &[ActionId]) -> bool {
    ids.iter().all(|id| p.action(id.as_str()).is_some_and(|a| a.status == ActionStatus::Done))
}

/// A starting run's board write is done: its first turn, once all the writes it waits for are (4.3.3, M3.23).
pub fn status_written(p: &mut Poster<'_>, card: Card, id: &ActionId, ctx: Ctx<'_>) {
    if card.stage != Stage::Starting {
        return;
    }
    if card.awaits.as_ref().is_some_and(|ids| ids.contains(id) && written(p, ids)) {
        first_turn(p, card, ctx);
    }
}

/// Headline of the engine's first specification request on a full card.
pub(super) const AGREE_HEADLINE: &str = "Agree the specification";

fn first_turn(p: &mut Poster<'_>, mut card: Card, ctx: Ctx<'_>) {
    let id = card.id.clone();
    card.stage = if card.mode == Mode::Report { Stage::Report } else { Stage::Brief };
    card.awaits = Some(Vec::new());
    p.put_card(card.clone());
    // A visual card opens at its baseline step first (fac-636); on a no-planner card the foreman receives the
    // specification request once its baseline ends or visual mode turns off (fac-688).
    if super::baseline::wanted(&card) {
        super::baseline::open(p, card.clone(), ctx);
        card = p.card(&id).unwrap_or(card);
        if card.no_planner {
            return;
        }
    }
    let (role, headline, text) = match card.mode {
        Mode::Report => (Role::Foreman, "Write the report", report_request(&id)),
        // Each plan is challenged at brief by the card's foreman before its approve starts the build.
        Mode::Full => {
            let text = first(&card) + &carrying(&card) + &super::baseline::first_note(&card, ctx.config);
            (super::facts::holder(&card), AGREE_HEADLINE, text)
        }
        Mode::Light | Mode::Review => return,
    };
    ask_agent(p, &id, role, headline, &text);
}

// The cards a full run carries, which its specification covers with its own (fac-563).
pub(super) fn carrying(card: &Card) -> String {
    let ids: Vec<&str> = card.carries.iter().map(|k| k.id.as_str()).collect();
    match ids.as_slice() {
        [] => String::new(),
        _ => format!(
            " This run carries {} too, shown after {} in your card: one specification, plan, branch and merge covers \
             them all.",
            ids.join(", "),
            card.id
        ),
    }
}

// A report card's request for its report, at its start.
fn report_request(id: &CardId) -> String {
    format!(
        "Write the report {id} asks for, from its card and the code. End with `report-ready`, its findings included."
    )
}

/// The scope of a specification ask before plan approval, shared by both brief holders.
pub(crate) const BRIEF_ASKS: &str = "Put in-scope defaults that follow established practice in the recap for `approve`. Raise a specification ask only for two defensible options with a material tradeoff or a proposed departure from the card or a standing decision.";

/// The first request of a full card: scout, agree the specification, then recap the plan for its foreman's challenge,
/// or, with no planner, for approval.
pub(super) fn first(card: &Card) -> String {
    let id = &card.id;
    if card.no_planner {
        return alone(card);
    }
    format!(
        "Agree {id}'s specification, as the `agreeing-a-specification` skill says. Scout first: read the card \
         and what it names, and give researcher seats the other questions the plan depends on. {BRIEF_ASKS} Raise these asks in your flagged hand (on a delegated or \
         self-sustained card the engine gives it to the project's orchestrator automatically; never send a \
         specification question as a message to the orchestrator). Direct request messages to the orchestrator are \
         for mechanical needs only. Answer a question of his in your flagged hand message, never as a decision nor in a \
         recap. Once nothing needs clarifying, recap it asking nothing, with the sketch of its shape in \
         the hand's `changes` and `code_shape`, in the skill's plain line forms, the long plan in its `message`, and \
         end `brief-ready`. \
         The card's foreman, seated with you at start, challenges it before any approve ask opens; \
         every later recap is challenged again. With no objections the engine offers the plan at once. \
         Otherwise settle them with the foreman by messages, as long as it takes, changing the plan for the \
         objections you take, then recap it: your messages to `foreman` and its to you pass in the card's \
         challenge place, which no window shows. The operator's words reach you in this spec room, the foreman his in \
         the build room. One it still holds after your refusal goes in your flagged hand message with both \
         positions, one at a time, at every delegation level (never decide it alone). An offered plan never \
         approves itself: it raises an `approve-brief` ask (given to the project's orchestrator automatically on \
         a delegated non-visual card) and waits for `approve` from the operator, or from the project's orchestrator \
         while that ask is given to it; a visual card's waits for the operator's own. At approval your work at brief \
         ends and the foreman takes the build."
    )
}

// A no-planner card's first request, to its foreman (fac-616, fac-688): no seat challenges its recap.
fn alone(card: &Card) -> String {
    let id = &card.id;
    let writes = if card.visual {
        "Edit no tracked repository files before approval. Follow the visual brief for edits to `.git/mork-mock/` after the baseline step."
    } else {
        "Write, edit and commit nothing before approval."
    };
    format!(
        "{id} has no planner: you hold its specification, at brief and after approval. Agree it as the \
         `agreeing-a-specification` skill says. Scout first: read the card and what it names, and give researcher \
         seats the other questions the plan depends on. {writes} {BRIEF_ASKS} Raise these asks in your flagged hand \
         (on a delegated or self-sustained card the engine gives it to the project's orchestrator automatically; \
         never send a specification question as a message to the orchestrator). Direct request messages to the \
         orchestrator are for mechanical needs only. Answer a question of his in your flagged hand message, never as a \
         decision nor in a recap. Once nothing needs clarifying, recap it asking nothing, with the sketch of its \
         shape in the hand's `changes` and `code_shape`, in the skill's plain line forms, the long plan in its \
         `message`, and end `brief-ready`. No seat challenges it: an offered plan never approves itself; it raises an `approve-brief` \
         ask (given to the project's orchestrator automatically on a delegated non-visual card) and waits for \
         `approve` from the operator, or from the project's orchestrator while that ask is given to it; a visual card's \
         waits for the operator's own. At approval your brief researchers are released, and the build request comes to you \
         in this session."
    )
}
