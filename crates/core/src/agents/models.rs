//! Changing a model keeps its seat's wall, tools, credentials and authority (fac-96, fac-115); what a seat runs is
//! its roster's row for where it stands (fac-399).

use mork_types::card::{Card, Mode};
use mork_types::config::{
    Cli, Config, Effort, MODEL_CATALOG, OPERATOR_MODELS, PickModel, ROSTER, ReviewKind, SEAT_DEFAULTS, Seat, SeatPhase,
    Standing, Wall, barred, cli_of_model,
};
use mork_types::reply::Role;
use mork_types::state::{Agent, Choice, Fresh, MessageKind, Sender};
use mork_types::view::{ModelChoice, Setting};
use mork_types::{AgentKey, CardId, Command};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::places::{self, MAIN};
use crate::post::Poster;
use crate::state::{Change, State};

// The models to pick from: every configured seat's, then the roster's and the catalog's, each once by CLI and model,
// under its first label (fac-246, fac-270, fac-399).
fn choices(config: &Config, by: By) -> Vec<ModelChoice> {
    let mut out: Vec<ModelChoice> = Vec::new();
    let seats = SEAT_DEFAULTS.iter().flat_map(|d| {
        Standing::ALL.into_iter().filter_map(move |at| config.seat_in(d.0, at)).map(|s| (s.cli, s.model, s.label))
    });
    let roster = ROSTER.iter().map(|r| (r.3, r.4.to_owned(), r.5.to_owned()));
    let catalog = MODEL_CATALOG.iter().chain(OPERATOR_MODELS.iter().filter(|_| by == By::Operator));
    let catalog = catalog.map(|(cli, model, label)| (*cli, (*model).to_owned(), (*label).to_owned()));
    let picks = config.picks.iter().flat_map(|p| [&p.researcher, &p.reviewer.inside, &p.reviewer.outside]).flatten();
    let picks = picks.map(|m| (m.cli, m.model.clone(), m.label.clone()));
    for (cli, model, label) in seats.chain(roster).chain(picks).chain(catalog) {
        if !out.iter().any(|m| m.cli == cli && m.model == model) {
            out.push(ModelChoice { cli, model, label });
        }
    }
    out
}

/// Where a seat stands for roster defaults, review classification and phase-bound picks.
pub fn standing(state: &State, a: &Agent) -> Standing {
    let Some(card) = places::card_of(state, &a.place).and_then(|c| state.cards.get(&c)) else {
        return Standing::WORK;
    };
    let review = card.mode == Mode::Review && matches!(a.seat.as_str(), "review-foreman" | "reviewer");
    Standing { phase: phase_of(card, a), review: review.then(|| crate::reviews::gate::kind(state, card)) }
}

fn phase_of(card: &Card, a: &Agent) -> SeatPhase {
    if a.seat.as_str() == "foreman" && card.mode == Mode::Report { SeatPhase::Report } else { SeatPhase::Work }
}

/// Expires the picks of a card's seats whose phase the card's change moves (fac-399), so a phase left and come back to
/// runs its row again: a pending pick is dropped, a launched choice unstamped, kept for its running turn and its switch.
pub fn rephase(p: &mut Poster<'_>, now: &Card) {
    let Some(was) = p.card(&now.id) else { return };
    let ours = |a: &&Agent| places::card_of(p.state, &a.place).as_ref() == Some(&now.id);
    let moved = |a: &Agent| phase_of(&was, a) != phase_of(now, a);
    let agents: Vec<Agent> =
        p.state.agents.values().filter(ours).filter_map(|a| p.agent(&a.key)).filter(moved).collect();
    for mut a in agents {
        a.pending = None;
        if let Some(c) = a.choice.as_mut() {
            c.phase = None;
        }
        p.put_agent(a);
    }
}

/// A new review round expires only configured choices; explicit and pending picks survive (fac-497).
pub fn reround(p: &mut Poster<'_>, id: &CardId) {
    for key in crate::reviews::gate::agents_of(p, id) {
        let Some(mut a) = p.agent(&key) else { continue };
        if !matches!(a.role, Role::Foreman | Role::Reviewer) || a.retiring.as_ref().is_some_and(|r| r.remove) {
            continue;
        }
        let Some(c) = a.choice.as_mut().filter(|c| c.by == "mork.toml") else { continue };
        c.phase = None;
        p.put_agent(a);
    }
}

/// Who chooses a model: the operator bypasses allowlists; a foreman uses the child's list. Every selection keeps its wall.
/// Any other sender keeps its own authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Operator,
    Foreman,
    Other,
}

impl By {
    /// The chooser a command's sender is, by its agent's role.
    pub fn of(state: &State, from: &Sender) -> Self {
        match from {
            Sender::Operator => Self::Operator,
            Sender::Agent(k) if state.agents.get(k).is_some_and(|a| a.role == Role::Foreman) => Self::Foreman,
            Sender::Agent(_) | Sender::Engine | Sender::Orchestrator(_) | Sender::Pa => Self::Other,
        }
    }

    // A stored choice's chooser, by its `by`: a card's foreman is `<card>/foreman`, its DM's `<card>/foreman/dm`.
    fn made(c: &Choice) -> Self {
        match c.by.as_str() {
            "operator" => Self::Operator,
            by if by.ends_with("/foreman") || by.ends_with("/foreman/dm") => Self::Foreman,
            _ => Self::Other,
        }
    }
}

fn restricted(a: &Agent, by: By) -> bool {
    by != By::Operator
        && (super::children::is_child(a) || (by == By::Foreman && matches!(a.role, Role::Researcher | Role::Reviewer)))
}

fn child_picks<'a>(config: &'a Config, a: &Agent, at: Standing) -> &'a [PickModel] {
    config.picks.as_ref().map_or(&[], |p| p.for_role(a.role.as_str(), at.review))
}

fn pick_list(a: &Agent, at: Standing) -> String {
    match (a.role, at.review) {
        (Role::Reviewer, Some(ReviewKind::Inside)) => "picks.reviewer.inside".to_owned(),
        (Role::Reviewer, Some(ReviewKind::Outside)) => "picks.reviewer.outside".to_owned(),
        _ => format!("picks.{}", a.role.as_str()),
    }
}

// Configured permissions never inherit choices from defaults or the catalog (fac-469).
fn candidates(config: &Config, a: &Agent, at: Standing, by: By) -> Vec<ModelChoice> {
    let configured = child_picks(config, a, at).iter().map(|m| ModelChoice {
        cli: m.cli,
        model: m.model.clone(),
        label: m.label.clone(),
    });
    let rest = choices(config, by).into_iter().filter(|_| !restricted(a, by));
    let mut out: Vec<ModelChoice> = Vec::new();
    for m in configured.chain(rest) {
        if !out.iter().any(|c| c.cli == m.cli && c.model == m.model) {
            out.push(m);
        }
    }
    out
}

/// The models offered to it where it stands, as `by` may choose them.
pub fn choices_for(state: &State, config: &Config, a: &Agent, by: By) -> Vec<ModelChoice> {
    offers(config, a, standing(state, a), by)
}

// An unwalled Claude seat's deny list exists only in Claude Code (fac-25, fac-402); agy's wall alone holds it (fac-657).
fn walls_kept(seat: &Seat, cli: Cli) -> bool {
    seat.wall != Wall::None || (cli != Cli::Agy && (seat.cli == Cli::Codex || cli == Cli::Claude))
}

// Fable and Astra are offered to the operator's own pick alone (fac-558, fac-615); a session already on one keeps it.
fn offers(config: &Config, a: &Agent, at: Standing, by: By) -> Vec<ModelChoice> {
    let Some(seat) = config.seat_in(&a.seat, at) else { return Vec::new() };
    let kept = |m: &ModelChoice| walls_kept(&seat, m.cli) && (by == By::Operator || !barred(&m.model));
    candidates(config, a, at, by).into_iter().filter(kept).collect()
}

// A chooser held to its picks chooses high or xhigh alone, whatever they list (fac-558); the operator's own pick takes every
// effort, max included (fac-615), and defaults and launched sessions run as configured.
const PICKED_EFFORTS: [Effort; 2] = [Effort::High, Effort::Xhigh];

fn efforts_of(a: &Agent, by: By) -> &'static [Effort] {
    let others = if restricted(a, by) { &PICKED_EFFORTS[..] } else { &Effort::OFFERED };
    if by == By::Operator { &Effort::ALL } else { others }
}

/// The available efforts; a restricted chooser's union is checked against the selected model at selection.
pub fn efforts_for(state: &State, config: &Config, a: &Agent, by: By) -> Vec<Effort> {
    let (efforts, at) = (efforts_of(a, by).iter().copied(), standing(state, a));
    let listed =
        |e: &Effort| child_picks(config, a, at).iter().any(|m| m.cli.takes(&m.model, *e) && m.efforts.contains(e));
    efforts.filter(|e| !restricted(a, by) || listed(e)).collect()
}

fn allowed(config: &Config, a: &Agent, at: Standing, cli: Cli, model: &str, effort: Effort, by: By) -> bool {
    cli.takes(model, effort)
        && efforts_of(a, by).contains(&effort)
        && (!restricted(a, by)
            || child_picks(config, a, at)
                .iter()
                .any(|m| m.cli == cli && m.model == model && m.efforts.contains(&effort)))
}

fn permissions(config: &Config, a: &Agent, at: Standing) -> Vec<(ModelChoice, String)> {
    offers(config, a, at, By::Foreman)
        .into_iter()
        .filter_map(|m| {
            let efforts: Vec<&str> = Effort::OFFERED
                .into_iter()
                .filter(|e| allowed(config, a, at, m.cli, &m.model, *e, By::Foreman))
                .map(Effort::as_str)
                .collect();
            (!efforts.is_empty()).then(|| (m, efforts.join(" or ")))
        })
        .collect()
}

/// The models and efforts its foreman may pick, preserving the seat's wall.
pub(super) fn pick_options(state: &State, config: &Config, a: &Agent) -> String {
    let at = standing(state, a);
    let models: Vec<String> = permissions(config, a, at)
        .into_iter()
        .map(|(m, efforts)| format!("{} ({}, {}) at {efforts}", m.label, m.model, m.cli.as_str()))
        .collect();
    if models.is_empty() {
        return format!("none: {} permits no model", pick_list(a, at));
    }
    let models = models.join("; ");
    if a.role == Role::Reviewer { format!("{}: {models}", pick_list(a, at)) } else { models }
}

// A pick launches while what it named is still offered where its agent stands.
fn offered(config: &Config, a: &Agent, at: Standing, c: &Choice) -> bool {
    let by = By::made(c);
    allowed(config, a, at, c.cli, &c.model, c.effort, by)
        && offers(config, a, at, by).iter().any(|m| m.cli == c.cli && m.model == c.model)
}

// Its seat where it stands with a selection's model and effort over it, offered or not: a session runs what it
// launched with, whatever the configuration says since.
fn with(config: &Config, a: &Agent, at: Standing, c: Option<&Choice>) -> Option<Seat> {
    let seat = config.seat_in(&a.seat, at)?;
    let Some(c) = c else { return Some(seat) };
    // Its label is looked up among every candidate: a session keeps a model no longer offered to it.
    let by = By::made(c);
    let label = candidates(config, a, at, by)
        .into_iter()
        .chain(choices(config, by))
        .find(|m| m.cli == c.cli && m.model == c.model);
    let label = label.map_or_else(|| c.model.clone(), |m| m.label);
    // The advisor is Claude Code's alone (fac-5).
    let advisor = seat.advisor.clone().filter(|_| c.cli == Cli::Claude);
    Some(Seat { cli: c.cli, model: c.model.clone(), label, effort: c.effort, advisor, ..seat })
}

/// The session's seat with its launched selection, including one the configuration has since dropped.
pub fn seat(config: &Config, a: &Agent) -> Option<Seat> {
    with(config, a, Standing::WORK, a.choice.as_ref())
}

// Phase changes expire picks; a new review round expires configured choices alone (fac-497).
fn next<'a>(config: &Config, a: &'a Agent, at: Standing) -> Option<&'a Choice> {
    let here = |c: &&Choice| c.phase == Some(at.phase);
    let picked = || a.pending.as_ref().filter(here).filter(|c| offered(config, a, at, c));
    picked().or_else(|| a.choice.as_ref().filter(here))
}

/// The next turn's seat, keeping a launched selection or using its unrestricted configured default.
pub fn launching(state: &State, config: &Config, a: &Agent) -> Option<Seat> {
    launching_at(config, a, standing(state, a))
}

// A seat configured or launched at an effort its model lacks runs no turn until an effort is picked.
fn launching_at(config: &Config, a: &Agent, at: Standing) -> Option<Seat> {
    with(config, a, at, next(config, a, at)).filter(|s| s.cli.takes(&s.model, s.effort))
}

fn setting_of(s: Seat) -> Setting {
    Setting { model: ModelChoice { cli: s.cli, model: s.model, label: s.label }, effort: s.effort }
}

/// Its session's model and effort, as the view shows them.
pub fn setting(state: &State, config: &Config, a: &Agent) -> Option<Setting> {
    with(config, a, standing(state, a), a.choice.as_ref()).map(setting_of)
}

/// What its next turn launches with, where its session runs another: a pick, or its roster's row in a new phase.
pub fn pending(state: &State, config: &Config, a: &Agent) -> Option<Setting> {
    let at = standing(state, a);
    let (next, runs) = (with(config, a, at, next(config, a, at))?, with(config, a, at, a.choice.as_ref())?);
    let same = next.cli == runs.cli && next.model == runs.model && next.effort == runs.effort;
    (!same).then(|| setting_of(next))
}

// A spawn can set a model choice before there is a session to replace; a model changes in its session (fac-637).
fn switches(a: &Agent, cli: Cli) -> bool {
    a.choice.as_ref().is_some_and(|c| c.cli != cli)
}

/// A switch at launch that gives the room one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Switch {
    pub from: Seat,
    pub to: Seat,
}

/// Records the next turn's selection, retained across config changes. A CLI change starts a fresh session.
pub(super) fn launch(state: &State, config: &Config, a: &mut Agent, now: i64) -> Option<Switch> {
    let at = standing(state, a);
    let to = launching_at(config, a, at)?;
    let from = switches(a, to.cli).then(|| seat(config, a)).flatten();
    let here = |c: &Choice| c.phase == Some(at.phase);
    let picked = a.pending.take().filter(|p| here(p) && offered(config, a, at, p));
    let kept = a.choice.as_ref().filter(|c| here(c)).map(|c| c.by.clone());
    let by = picked.map(|p| p.by).or(kept).unwrap_or_else(|| "mork.toml".to_owned());
    let phase = Some(at.phase);
    a.choice = Some(Choice { cli: to.cli, model: to.model.clone(), effort: to.effort, by: by.clone(), phase });
    let from = from?;
    if a.session.is_some() || a.session_started {
        let replaces = a.session.clone().or_else(|| a.fresh.as_ref().and_then(|f| f.replaces.clone()));
        super::fresh::replace(state, a, Fresh { replaces, by, at: now, pending: false, vendor: Some(to.cli) });
    }
    Some(Switch { from, to })
}

// The room's line for a switch, in its card's room or its own place: the conversation before it stays where it is.
fn switched(key: &AgentKey, s: &Switch, card: bool) -> String {
    let side = |x: &Seat| format!("{} ({}, {})", cli_name(x.cli), x.label, x.effort.as_str());
    let reads = if card { ", which reads the card first" } else { "" };
    format!(
        "`{key}` switched from {} to {}: this turn starts a fresh session{reads}. The conversation before it stays.",
        side(&s.from),
        side(&s.to)
    )
}

/// A switch's line, as `switched` words it, posted when its turn is recorded.
pub(super) fn switch_line(state: &State, key: &AgentKey, s: &Switch, ctx: Ctx<'_>, d: &mut Decision) {
    let Some(a) = state.agents.get(key) else { return };
    let card = places::card_of(state, &a.place);
    let place = card.as_ref().map_or_else(|| a.place.to_string(), |c| c.as_str().to_owned());
    let text = switched(key, s, card.is_some());
    let mut p = crate::post::Poster::new(state, ctx.now);
    p.post(&place, Sender::Engine, Some("operator"), MessageKind::Engine, "", &text);
    p.finish(d);
    d.journal(
        "pick.switch",
        card.as_ref().map(|c| c.as_str()),
        json!({"agent": key.as_str(), "to": s.to.cli.as_str()}),
    );
}

/// A CLI's name in prose.
pub(super) fn cli_name(cli: Cli) -> &'static str {
    match cli {
        Cli::Claude => "Claude",
        Cli::Codex => "Codex",
        Cli::Agy => "agy",
    }
}

// The model named, on the CLI named if any: exactly one, else why not.
fn model_of(
    config: &Config,
    a: &Agent,
    at: Standing,
    want: &str,
    cli: Option<&str>,
    by: By,
) -> Result<ModelChoice, String> {
    let cli = match cli.map(str::trim) {
        None => None,
        Some(w) => Some(Cli::parse(w).ok_or_else(|| format!("`{w}` isn't a CLI: claude, codex or agy"))?),
    };
    if by != By::Operator && barred(want) {
        return Err(retired(want));
    }
    let models = offers(config, a, at, by);
    let on_cli = |m: &&ModelChoice| cli.is_none_or(|c| m.cli == c);
    let mut found: Vec<&ModelChoice> = models.iter().filter(|m| m.model == want).filter(on_cli).collect();
    // The docs say "Sonnet" and "Opus": a label's first word picks its model when no id is named (fac-255).
    let alias = found.is_empty().then(|| {
        let word = |m: &&ModelChoice| m.label.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(want));
        models.iter().filter(word).filter(on_cli).collect::<Vec<_>>()
    });
    if let Some(alias) = alias.filter(|a| !a.is_empty()) {
        if alias.iter().any(|m| m.model != alias[0].model) {
            let ids: Vec<&str> = alias.iter().map(|m| m.model.as_str()).collect();
            return Err(format!("`{want}` names several models: name one by its id, {}", ids.join(" or ")));
        }
        found = alias;
    }
    if found.is_empty()
        && let Some(e) = withheld(config, a, at, by, want, cli)
    {
        return Err(e);
    }
    let list: Vec<&str> = models.iter().map(|m| m.model.as_str()).collect();
    match found.as_slice() {
        [m] => Ok((*m).clone()),
        [] if restricted(a, by) => Err(format!(
            "`{want}` is not permitted by {} on this CLI: an absent or empty list permits no model; the models are {}",
            pick_list(a, at),
            list.join(", ")
        )),
        [] => {
            let on = cli.map(|c| format!(" on {}", cli_name(c))).unwrap_or_default();
            Err(format!(
                "no seat runs `{want}`{on}: the models are {}; a new one is configured in mork.toml first",
                list.join(", ")
            ))
        }
        many => many.iter().find(|m| Some(m.cli) == cli_of_model(&m.model)).map(|m| (*m).clone()).ok_or_else(|| {
            format!("seats run `{want}` on more than one CLI: name its CLI, `--cli claude`, `codex` or `agy`")
        }),
    }
}

fn retired(want: &str) -> String {
    format!("`{want}` is the operator's own pick alone: Fable and Astra take no spawn, start or other pick")
}

// Why a model named, by id or its label's first word, isn't offered: a wall bars it, or it is the operator's alone (fac-615).
fn withheld(config: &Config, a: &Agent, at: Standing, by: By, want: &str, cli: Option<Cli>) -> Option<String> {
    let word = |m: &ModelChoice| m.label.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(want));
    let named = |m: &ModelChoice| cli.is_none_or(|c| m.cli == c) && (m.model == want || word(m));
    let hidden: Vec<ModelChoice> = candidates(config, a, at, by).into_iter().filter(named).collect();
    if hidden.iter().any(|m| by == By::Operator || !barred(&m.model)) {
        return Some(format!("`{}` runs unwalled, its deny list Claude Code's alone: choose a Claude model", a.key));
    }
    (!hidden.is_empty()).then(|| retired(want))
}

const OFFERED_EFFORTS: &str = "low, medium, high or xhigh";
const OPERATOR_EFFORTS: &str = "low, medium, high, xhigh or max";

/// Validates spawn and pick against the seat's wall and one CLI/model/effort entry; the operator bypasses the allowlist.
pub fn selection(
    config: &Config,
    a: &Agent,
    at: Standing,
    model: Option<&str>,
    effort: Option<&str>,
    cli: Option<&str>,
    by: By,
) -> Result<Setting, String> {
    let now = with(config, a, at, next(config, a, at)).ok_or_else(|| format!("no seat `{}`", a.seat))?;
    let m = match model.map(str::trim) {
        None if cli.is_some() => return Err("`cli` names a model's CLI: name the model too".into()),
        None => model_of(config, a, at, &now.model, Some(now.cli.as_str()), by)?,
        Some(want) => model_of(config, a, at, want, cli, by)?,
    };
    let names = if by == By::Operator { OPERATOR_EFFORTS } else { OFFERED_EFFORTS };
    let effort = match effort.map(str::trim) {
        None => now.effort,
        Some(word) => Effort::parse(word).ok_or_else(|| format!("`{word}` isn't an effort: {names}"))?,
    };
    if !m.cli.takes(&m.model, effort) {
        let list: Vec<&str> = m.cli.efforts(&m.model).iter().map(|e| e.as_str()).collect();
        let valid = list.split_last().map_or_else(String::new, |(l, r)| format!("{} or {l}", r.join(", ")));
        return Err(format!("{} takes no `{}`: name an effort, {valid}", m.label, effort.as_str()));
    }
    if by != By::Operator && !Effort::OFFERED.contains(&effort) {
        let e = effort.as_str();
        return Err(format!("`{e}` is the operator's own pick alone: name an effort, {OFFERED_EFFORTS}"));
    }
    if !allowed(config, a, at, m.cli, &m.model, effort, by) {
        let efforts = permissions(config, a, at)
            .into_iter()
            .find(|(model, _)| model.cli == m.cli && model.model == m.model)
            .map_or_else(|| "none".to_owned(), |(_, efforts)| efforts);
        let (e, model, cli, list) = (effort.as_str(), &m.model, m.cli.as_str(), pick_list(a, at));
        return Err(format!("`{e}` is not permitted for `{model}` on {cli} by {list}; permitted efforts: {efforts}"));
    }
    Ok(Setting { model: m, effort })
}

/// Queues a pick for the agent's next turn in this phase. Picking its current selection records who picked it and
/// clears any pending pick.
pub(super) fn pick(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    let Command::Pick { agent: name, model, effort, cli } = cmd else { return Decision::default() };
    let name = name.trim();
    if name.is_empty() || (model.is_none() && effort.is_none()) {
        return Decision::refused("name the agent and a model, an effort or both: `pick ex-550/foreman --effort high`");
    }
    let key = if name.contains('/') { AgentKey::new(name) } else { AgentKey::new(format!("{MAIN}/{name}")) };
    let Some(agent) = state.agents.get(&key) else { return Decision::refused(format!("no agent `{key}`")) };
    let by = match from {
        Sender::Agent(k) => k.as_str().to_owned(),
        Sender::Operator | Sender::Engine => "operator".to_owned(),
        Sender::Orchestrator(_) | Sender::Pa => from.label(),
    };
    let at = standing(state, agent);
    let (model, effort, cli) = (model.as_deref(), effort.as_deref(), cli.as_deref());
    let Setting { model: m, effort } = match selection(ctx.config, agent, at, model, effort, cli, By::of(state, from)) {
        Ok(s) => s,
        Err(e) => return Decision::refused(e),
    };
    let mut a = agent.clone();
    let runs = a.choice.as_mut().filter(|c| c.phase == Some(at.phase));
    let same = runs.as_ref().is_some_and(|s| s.cli == m.cli && s.model == m.model && s.effort == effort);
    if same && let Some(c) = runs {
        c.by = by.clone();
    }
    let (needs_choice, phase) = (agent.choice.is_none(), Some(at.phase));
    a.pending = (!same || needs_choice).then(|| Choice { cli: m.cli, model: m.model.clone(), effort, by, phase });
    let fresh = a.pending.is_some() && agent.choice.as_ref().is_some_and(|c| c.cli != m.cli);
    let body = json!({"agent": key.as_str(), "cli": m.cli.as_str(), "model": m.model, "effort": effort.as_str(),
                      "pending": a.pending.is_some(), "fresh": fresh});
    let mut d = Decision::ok(json!({"pick": body.clone()}));
    d.journal("pick", places::card_of(state, &a.place).as_ref().map(|c| c.as_str()), body);
    d.changes.push(Change::PutAgent(a));
    d
}
