//! Child seats report to the parent that tasked them (fac-94, fac-95).

use mork_types::ask::Color;
use mork_types::card::{Card, Mode, Stage};
use mork_types::command::Command;
use mork_types::reply::{Intent, Outcome, Reply, Role};
use mork_types::review::Verdict;
use mork_types::state::{Agent, Choice, Job, MessageKind, Owed, Sender};
use mork_types::{AgentKey, CardId, PlaceId, SeatName};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::places::{self, Who};
use crate::post::Poster;
use crate::state::State;

const ROLES: [Role; 3] = [Role::Researcher, Role::Reviewer, Role::Baseline];

/// The role a child's name says (`reviewer-2`, `researcher-1`), if it is one.
pub fn role_of(name: &str) -> Option<Role> {
    let (role, n) = name.rsplit_once('-')?;
    ROLES.into_iter().find(|r| r.as_str() == role).filter(|_| numbered(n))
}

/// Whether a name is a child's: one of a role this build has, or a mocker's, whose DMs stay as history (fac-591).
pub fn child_name(name: &str) -> bool {
    role_of(name).is_some() || name.strip_prefix("mocker-").is_some_and(numbered)
}

/// Whether a saved numbered builder used its own DM (fac-660).
pub(crate) fn builder_dm_name(name: &str) -> bool {
    name.strip_prefix("builder-").is_some_and(numbered)
}

fn numbered(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !n.starts_with('0')
}

/// A card's child by its name, once spawned.
pub fn named(state: &State, card: &CardId, name: &str) -> Option<Who> {
    role_of(name)?;
    let a = state.agents.get(format!("{card}/{name}").as_str()).filter(|a| is_child(a))?;
    places::who_of(state, &a.key)
}

/// A card's children, in the order they were spawned.
pub fn of_card(state: &State, card: &CardId) -> Vec<Who> {
    let mut kids: Vec<(Role, u32, Who)> = state
        .agents
        .values()
        .filter(|a| is_child(a) && places::card_of(state, &a.place).as_ref() == Some(card))
        .filter_map(|a| Some((a.role, number(&a.key)?, places::who_of(state, &a.key)?)))
        .collect();
    kids.sort_by_key(|(r, n, _)| (*n, r.as_str()));
    kids.into_iter().map(|(_, _, w)| w).collect()
}

/// Whether an agent is a numbered child, a researcher, reviewer or baseline seat: the planner, its foreman's
/// unnumbered child (fac-633), is none.
pub fn is_child(a: &Agent) -> bool {
    a.parent.is_some() && ROLES.contains(&a.role)
}

fn number(key: &AgentKey) -> Option<u32> {
    key.as_str().rsplit_once('-')?.1.parse().ok()
}

// The next free name in a role: one more than the last spawned, or than a finding's raiser, gone or not, never
// reused: a finding is verified by its own raiser alone (fac-111), and a DM's first task is its reviewer's topic.
fn next_name(state: &State, card: &Card, role: Role) -> String {
    let id = &card.id;
    let prefix = format!("{}-", role.as_str());
    let owners = format!("{id}/{prefix}");
    let raised = state.cards.get(id).and_then(|c| c.review.as_ref()).map(|r| r.raised.as_slice()).unwrap_or_default();
    let raisers = raised.iter().filter_map(|x| x.by.strip_prefix(&prefix)?.parse().ok());
    let running = state.jobs.values().filter(|j| j.settled.is_none() && j.owner.as_str().starts_with(&owners));
    // A released researcher's or baseline seat's first task still reserves its DM for this run; a reviewer's, whose
    // topic is its DM's first task and whose history spans a review card's runs, for good.
    let run = matches!(role, Role::Researcher | Role::Baseline);
    let research = state
        .messages
        .iter()
        .rev()
        .take_while(|m| role == Role::Reviewer || (run && m.seq >= card.first_seq))
        .filter_map(|m| m.place.as_str().strip_prefix(&owners)?.strip_suffix("/dm")?.parse().ok());
    let last = state
        .agents
        .values()
        .filter(|a| is_child(a) && a.role == role && a.key.as_str().starts_with(&owners))
        .filter_map(|a| number(&a.key))
        .chain(running.filter_map(|j| number(&j.owner)))
        .chain(raisers)
        .chain(research)
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", last + 1)
}

// Reviewers and baseline seats need the corresponding card step.
pub(super) fn spawnable(card: &Card, role: Role) -> Result<(), String> {
    let id = &card.id;
    if !card.stage.is_live() {
        return Err(format!("{id} isn't live"));
    }
    match role {
        Role::Researcher => Ok(()),
        Role::Reviewer if card.mode == Mode::Review => Ok(()),
        Role::Reviewer => Err(format!(
            "{id} is a {} card: only a review card's foreman spawns reviewers",
            crate::lifecycle::facts::mode_word(card.mode)
        )),
        Role::Baseline if crate::lifecycle::mock::baseline(card) => Ok(()),
        Role::Baseline => Err(format!(
            "{id} isn't at its baseline step: a foreman spawns a baseline seat only on its visual card, while the \
             engine's request to build the baseline stands"
        )),
        Role::Planner | Role::Foreman | Role::Echo => {
            Err(format!("a foreman spawns a researcher, a reviewer or a baseline seat, not a {}", role.as_str()))
        }
    }
}

// Only one baseline seat writes the mock at a time.
fn one_seat(state: &State, card: &Card, role: Role) -> Result<(), String> {
    let name = |w: Who| w.key.as_str().rsplit('/').next().unwrap_or_default().to_owned();
    match role {
        Role::Baseline => seated(state, &card.id, Role::Baseline).map_or(Ok(()), |b| {
            Err(format!("{} already has its baseline seat `{}`: send it on by name", card.id, name(b)))
        }),
        Role::Planner | Role::Foreman | Role::Reviewer | Role::Researcher | Role::Echo => Ok(()),
    }
}

// A card's child of a role still in its seat, not retiring.
fn seated(state: &State, card: &CardId, role: Role) -> Option<Who> {
    let active = |w: &Who| w.role == role && state.agents.get(&w.key).is_some_and(|a| a.retiring.is_none());
    of_card(state, card).into_iter().find(active)
}

/// A card's new child, reading its run's news from its own DM on.
pub(crate) fn child(state: &State, c: &Card, role: Role) -> (Who, Agent) {
    let name = next_name(state, c, role);
    let place = PlaceId::new(format!("{}/{name}/dm", c.id));
    let key = AgentKey::new(format!("{}/{name}", c.id));
    let who = Who { key, seat: SeatName::new(role.as_str()), role, place };
    let mut agent = crate::post::new_agent(&who);
    let parent = crate::lifecycle::facts::holder(c);
    agent.parent = Some(crate::messages::requests::agent_of(&c.id, parent).key);
    agent.seen = c.first_seq;
    (who, agent)
}

/// Gives a child a task in its DM, as a request that wakes it.
pub(crate) fn task(p: &mut Poster<'_>, who: &Who, from: &Sender, text: &str) {
    let name = who.key.as_str().rsplit('/').next().unwrap_or_default().to_owned();
    p.post(who.place.as_str(), from.clone(), Some(&name), MessageKind::Request, "", text);
    if let Some(m) = p.last_message() {
        m.intent = Some(Intent::Request);
    }
    p.wake(who, from);
}

/// Creates a child before posting its task; a reviewer may use its configured default, while explicit choices
/// must keep the seat's wall and the sender's model and effort permissions.
pub(super) fn spawn(state: &State, from: &Sender, cmd: &Command, ctx: Ctx<'_>) -> Decision {
    let Command::Spawn { card, role, task: task_text, model, effort, cli, topic } = cmd else {
        return Decision::refused("not a spawn command");
    };
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(c) = state.cards.get(&id) else { return Decision::refused(format!("no card `{id}`")) };
    if let Err(e) = spawnable(c, *role).and_then(|()| one_seat(state, c, *role)) {
        return Decision::refused(e);
    }
    if task_text.trim().is_empty() {
        return Decision::invalid("`task` is empty: a child starts on its task");
    }
    // One line, whatever its words hold: the room list shows it beside the reviewer's name.
    let topic = topic.as_deref().map(mork_types::chat::line);
    match (*role, topic.as_deref()) {
        (Role::Reviewer, None | Some("")) => {
            return Decision::refused("a reviewer needs its topic: `spawn --topic '<topic>'`, in your own words");
        }
        (Role::Reviewer, Some(_)) => {}
        (_, Some(_)) => return Decision::refused("only a reviewer has a topic: drop `--topic`"),
        (_, None) => {}
    }
    let (who, mut agent) = child(state, c, *role);
    let Sender::Agent(parent) = from else { return Decision::refused("a child needs a seat to report to") };
    agent.parent = Some(parent.clone());
    agent.topic = topic;
    agent.choice = match spawn_choice(state, from, &agent, c, (model.as_deref(), effort.as_deref(), cli.as_deref()), ctx) {
        Ok(choice) => choice,
        Err(e) => return Decision::refused(e),
    };
    let mut p = Poster::for_ctx(state, ctx);
    p.put_agent(agent);
    task(&mut p, &who, from, task_text.trim());
    p.journal("child.spawned", Some(id.as_str()), json!({"agent": who.key.as_str(), "role": role.as_str()}));
    let mut d = Decision::ok(json!({"spawn": {"agent": who.key.as_str(), "place": who.place.as_str()}}));
    p.finish(&mut d);
    d
}

fn spawn_choice(state: &State, from: &Sender, agent: &Agent, card: &Card,
    (model, effort, cli): (Option<&str>, Option<&str>, Option<&str>), ctx: Ctx<'_>) -> Result<Option<Choice>, String> {
    let at = super::models::standing(state, agent);
    let selected = model.is_some() || effort.is_some() || cli.is_some();
    if agent.role == Role::Researcher && crate::lifecycle::lookback::automatic(card) {
        if selected {
            return Err("the engine chooses this auto pass's researchers: drop `--model`, `--effort` and `--cli`".into());
        }
        return crate::lifecycle::trigger::best(state, "researcher", at, ctx)
            .map(|pair| Some(pair.choice(at.phase)))
            .ok_or_else(|| "no quota pool is open for a researcher now".into());
    }
    if agent.role == Role::Baseline && selected {
        return Err("a baseline seat runs on its configured default, `[seats.baseline]`: drop `--model`, `--effort` and `--cli`".into());
    }
    if !selected && matches!(agent.role, Role::Reviewer | Role::Baseline) {
        return Ok(None);
    }
    if model.is_none() || effort.is_none() {
        return Err("choose this child's model and effort: `spawn --model <model> --effort <effort>`".into());
    }
    let s = super::models::selection(ctx.config, agent, at, model, effort, cli, super::models::By::of(state, from))?;
    Ok(Some(Choice { cli: s.model.cli, model: s.model.model, effort: s.effort, by: from.label(), phase: Some(at.phase) }))
}

pub(super) const EMPTY_REVIEW: &str = "your review-fail has empty findings. Put each blocking finding in `findings`, or repeat `review-fail` with empty `findings` to confirm the failure";

pub(super) fn needs_confirmation(a: &Agent, job: &Job, r: &Reply) -> bool {
    a.role == Role::Reviewer
        && r.outcome == Outcome::ReviewFail
        && r.findings.is_empty()
        && (job.corrects.is_none() || job.corrects != a.confirm_empty_review)
}

/// Reports a child's finished, blocked, asking or red reply to its parent's live turn or wakes it in the parent's
/// room. Nothing submits the card, decides a review or asks the operator (fac-94, decision 1; fac-95; fac-99).
pub fn replied(p: &mut Poster<'_>, card: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    let Some(a) = p.agent(&who.key).filter(is_child) else { return false };
    let Some(parent) = a.parent.clone() else { return false };
    if a.retiring.as_ref().is_some_and(|r| r.remove) && who.role == Role::Researcher {
        return true;
    }
    let finished = matches!(r.outcome, Outcome::Built | Outcome::Reported | Outcome::ReviewPass | Outcome::ReviewFail);
    let asks = r.hand.ask || r.hand.color == Color::Red;
    let done = match r.outcome {
        Outcome::Built | Outcome::Reported => "finished its task",
        Outcome::ReviewPass => "passed its topic",
        Outcome::ReviewFail => "failed its topic",
        Outcome::Blocked => "is blocked",
        Outcome::Working | Outcome::BriefReady | Outcome::ReportReady | Outcome::None if asks => "is at its task",
        Outcome::Working | Outcome::BriefReady | Outcome::ReportReady | Outcome::None => return true,
    };
    let Some(recipient) = places::who_of(p.state, &parent) else { return true };
    let name = who.key.as_str().rsplit('/').next().unwrap_or_default().to_owned();
    let round = reported_round(card, job).filter(|_| who.role == Role::Reviewer);
    if let Some(n) = round.filter(|_| matches!(r.outcome, Outcome::ReviewPass | Outcome::ReviewFail)) {
        crate::reviews::history::record(p, &card.id, n, &name, r);
    }
    let text = report_text(&name, done, r);
    let from = Sender::Agent(who.key.clone());
    // In its parent's room: the planner's researchers' in the spec room, others' in the build room (fac-633).
    let seq =
        p.post(recipient.place.as_str(), from.clone(), Some(recipient.role.as_str()), MessageKind::Request, "", &text);
    // A question, or what a blocked child needs, wants the foreman's answer; a plain report, none.
    let intent = if asks || r.outcome == Outcome::Blocked { Intent::Request } else { Intent::Response };
    if let Some(m) = p.last_message() {
        m.intent = Some(intent);
        m.images = r.hand.images.clone();
    }
    if p.deliver_report(&recipient, &who.key, seq) {
        return true;
    }
    // A finished report is a completion (fac-314), a reviewer's also covered by its round's verdict (fac-277);
    // a question never is, but a verdict's red hand alone asks nothing (fac-358).
    let verdict = matches!(r.outcome, Outcome::ReviewPass | Outcome::ReviewFail) && !r.hand.ask;
    if finished && (!asks || verdict) {
        // A sibling still at work holds it, so its foreman wakes once for them all (fac-528).
        if !super::held::hold(p, &recipient, &who.key, round) {
            p.wake_for_report(&recipient, &from, round);
        }
    } else {
        p.wake(&recipient, &from);
    }
    true
}

fn report_text(name: &str, done: &str, r: &Reply) -> String {
    let mut text = format!("{name} {done}: {}", r.hand.headline);
    if !r.hand.ask && !r.hand.message.is_empty() {
        text.push_str(&format!("\n\n{}", r.hand.message));
    }
    // Its question is its foreman's to answer or carry to the operator, so it travels whole.
    if r.hand.ask {
        text.push_str(&format!("\n\nIt asks: {}", r.hand.ask_words().trim()));
    }
    if !r.findings.is_empty() {
        text.push_str(&format!(
            "\n\nIts findings ({}):\n\n{}",
            r.findings.len(),
            crate::lifecycle::fixes::listed(&r.findings)
        ));
    }
    text
}

/// Ends the specification holder's brief researchers when its plan is approved: its planner's, or a no-planner card's
/// foreman's (fac-616).
pub(crate) fn release_researchers(p: &mut Poster<'_>, holder: &AgentKey, config: &mork_types::config::Config) {
    for a in p.state.agents.values().filter(|a| a.role == Role::Researcher && a.parent.as_ref() == Some(holder)) {
        super::closing::retire(p, &a.key, true, config);
    }
}

/// Ends a card's baseline seats when its baseline step ends (fac-636); with `stop`, visual mode turned off, their
/// running turns are stopped too. A seat already retiring keeps its closing turn.
pub(crate) fn release_baselines(p: &mut Poster<'_>, card: &CardId, stop: bool, config: &mork_types::config::Config) {
    let mine = |a: &&Agent| {
        a.role == Role::Baseline && a.retiring.is_none() && places::card_of(p.state, &a.place).as_ref() == Some(card)
    };
    let keys: Vec<AgentKey> = p.state.agents.values().filter(mine).map(|a| a.key.clone()).collect();
    for key in keys {
        if stop
            && crate::lifecycle::brief::stop_brief_turns(p, &key)
            && let Some(mut a) = p.agent(&key).filter(|a| a.session_started)
        {
            a.last_turn_ms = Some(p.now);
            p.put_agent(a);
        }
        super::closing::retire(p, &key, true, config);
    }
}

/// The round `who`'s turn was given when it is a reviewer's and still the one under way (fac-325).
pub(super) fn job_round(p: &Poster<'_>, who: &Who, job: &Job) -> Option<u32> {
    let card = places::card_of(p.state, &who.place).and_then(|c| p.card(&c))?;
    reported_round(&card, job).filter(|_| who.role == Role::Reviewer)
}

// The round a reviewer's turn was given, when it is still the one under way: the latest asked before the news
// the turn read, on the commit it was asked about. Any doubt is none, an ordinary wake.
fn reported_round(card: &Card, job: &Job) -> Option<u32> {
    let review = card.review.as_ref()?;
    let (seen, head) = (job.seen?, job.head.as_ref()?);
    let given = review.rounds.iter().rev().find(|x| x.asked.is_some_and(|a| a < seen) && &x.commit == head)?;
    review.open().filter(|o| o.n == given.n).map(|o| o.n)
}

/// Whether the wake `a` is owed was only reviewer reports of a round that now has its verdict (fac-277): it
/// is dropped then, the reports staying in history.
pub(super) fn covered_by_verdict(p: &Poster<'_>, a: &mut Agent) -> bool {
    let Some(n) = a.report_round else { return false };
    let card = crate::messages::places::card_of(p.state, &a.place).and_then(|c| p.card(&c));
    let closed = card.and_then(|c| c.review).is_some_and(|r| {
        r.rounds.iter().any(|x| x.n == n && matches!(x.verdict, Some(Verdict::Passed | Verdict::Failed)))
    });
    if !closed {
        return false;
    }
    drop_wake(a);
    true
}

/// A report card's foreman's accepted `report-ready` drops the wake only its children's completion reports
/// caused (fac-314): they stay in history.
pub(super) fn report_submitted(p: &Poster<'_>, a: &mut Agent) -> bool {
    let card = crate::messages::places::card_of(p.state, &a.place).and_then(|c| p.card(&c));
    if !a.completion_wake || a.role != Role::Foreman || !card.is_some_and(|c| c.stage == Stage::Report) {
        return false;
    }
    drop_wake(a);
    true
}

/// An accepted `built` drops the wake only the engine's "Build it" request caused (fac-328), the card still at
/// build while it bundles: the request stays in history. Only an accepted one sets `built_pending`.
pub(super) fn build_answered(p: &Poster<'_>, a: &mut Agent) -> bool {
    let card = crate::messages::places::card_of(p.state, &a.place).and_then(|c| p.card(&c));
    if !a.build_wake || !card.is_some_and(|c| c.gates.built_pending) {
        return false;
    }
    drop_wake(a);
    true
}

/// The same wake is dropped once the card has left build (fac-328): `Poster::finish` asks of each agent of a card
/// a decision moved out of it.
pub fn build_left(a: &mut Agent) -> bool {
    if !a.build_wake {
        return false;
    }
    drop_wake(a);
    true
}

fn drop_wake(a: &mut Agent) {
    // Held reports merged into this wake are dropped with it; ones still held for children at work stay (fac-528).
    if a.held_reports.is_some_and(|h| h.waking) {
        a.held_reports = None;
    }
    a.report_round = None;
    a.completion_wake = false;
    a.build_wake = false;
    if matches!(a.owed, Some(Owed::Wake)) {
        a.owed = None;
    }
    a.wake_after = false;
}

/// What a child's new session is told of its foreman and its folder.
pub(super) fn line(a: &Agent) -> Option<String> {
    let parent = a.parent.as_ref().filter(|_| is_child(a))?;
    let card = parent.as_str().split('/').next().unwrap_or_default();
    let room = if parent.as_str().ends_with("/planner") { format!("{card}'s spec") } else { format!("{card}'s") };
    let (ends, folder) = match a.role {
        Role::Researcher => (
            "`reported` or `blocked`",
            "You work in a folder of your own; the card's clone, at `$MORK_CLONE`, is yours to read, never to write."
                .to_owned(),
        ),
        Role::Reviewer => (
            "`review-pass`, `review-fail` or `blocked`",
            format!(
                "You review in the card's clone, at the commit under review, beside its other reviewers: run what you need there, and change no tracked file. Save each executable proof there under `.git/mork/proofs/`, named by its finding, and keep that path out of the finding's proof text; your foreman puts the ones it keeps into the round's folder there, and you rerun an earlier round's from its own `round-<n>/` folder. {}",
                crate::reviews::verdict::PROOF_RULE
            ),
        ),
        Role::Baseline => (
            "`built` or `blocked`",
            format!(
                "You write only the mock's folder, `{}/` in the card's clone; the rest of the clone is yours to read.",
                crate::lifecycle::mock::FOLDER
            ),
        ),
        Role::Planner | Role::Foreman | Role::Echo => return None,
    };
    Some(format!(
        "You work for `{parent}` alone: your {ends} reports to it, in {room} room, and submits nothing. It gives you later tasks here, by your name. {folder}"
    ))
}
