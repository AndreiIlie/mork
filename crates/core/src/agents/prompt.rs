//! Fresh sessions need their rules and role; resumed sessions need only live context and news
//! because material changes arrive as announcements (fac-14, fac-88).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use mork_types::card::{Card, Mode};
use mork_types::config::Config;
use mork_types::reply::Role;
use mork_types::state::{Agent, Message, MessageKind};

use crate::decide::Ctx;
use crate::messages::announce::{self, Heard};
use crate::messages::places;
use crate::state::State;

mod carried;
pub(crate) mod held;
mod rerun;
mod specification;

use held::Sections;
pub(super) use rerun::{Asked, continuation, rerun};
use specification::{FOREMAN_SPECIFICATION, planner_foreman, specification};

pub use crate::maintenance::manuals::{CONTRACT, role_file};

/// Whether an agent's next woken turn resumes its session: one that started, under a known id.
pub(super) fn resumes(a: &Agent) -> bool {
    a.session_started && a.session.is_some()
}

/// A woken agent's prompt, a new session's whole start or a resumed one's follow-up, with the sections it sends.
pub(super) fn turn(state: &State, a: &Agent, ctx: Ctx<'_>, codex_exec: bool) -> (String, BTreeMap<String, String>) {
    if a.role == Role::Echo {
        return (news(state, a), BTreeMap::new());
    }
    let resumed = resumes(a);
    // A turn through `codex exec` can't rely on its session: it sends everything.
    let mut s = Sections::of(a, resumed && !codex_exec);
    let text = if resumed { follow_up(state, a, ctx, &mut s) } else { startup(state, a, ctx, &mut s) };
    (text, s.staged)
}

fn startup(state: &State, a: &Agent, ctx: Ctx<'_>, s: &mut Sections<'_>) -> String {
    let mut out = String::new();
    for part in [crate::maintenance::manuals::mork_for(a.role).as_str(), role_file(a.role)] {
        out.push_str(part.trim());
        out.push_str("\n\n");
    }
    let _ = write!(
        out,
        "# This session\n\n## You\n\nYou are `{}`, the {}, in {}.\n{}\n",
        a.key,
        a.role.as_str(),
        place_words(state, &a.place),
        recipient_line(state, a, ctx)
    );
    let (preview, mock) =
        (crate::lifecycle::preview::line(state, a, ctx.config), crate::lifecycle::mock::line(state, a, ctx.config));
    for line in [super::fresh::line(a), super::children::line(a), preview, mock].into_iter().flatten() {
        let _ = writeln!(out, "{line}");
    }
    if let Some(c) = card_of(state, a) {
        let decisions_only = a.role == Role::Reviewer || a.fresh.as_ref().is_some_and(|f| f.by == "engine");
        card(state, c, decisions_only, &mut out);
        // A gate's review reads the specification of the card it gates, none of its discussion (fac-95).
        if let Some(s) = under_review(state, c) {
            out.push_str("\n## Under review\n\nThe card this review card gates, as its specification was agreed:\n");
            card(state, s, true, &mut out);
        }
    }
    let project = card_of(state, a).map(|c| c.project.as_str());
    out.push_str(&crate::memory::index(state, project, ctx.config.mork.project.as_ref()));
    live(state, a, ctx, s, &mut out);
    let outcomes: Vec<&str> = super::reply::outcomes(state, a).iter().map(|o| o.as_str()).collect();
    let _ = write!(
        out,
        "\n## The reply\n\n{CONTRACT}\n\nYour outcomes: {}. `schema` is {}.\n",
        outcomes.join(", "),
        mork_types::reply::SCHEMA_VERSION
    );
    out
}

fn follow_up(state: &State, a: &Agent, ctx: Ctx<'_>, s: &mut Sections<'_>) -> String {
    let mut out = String::new();
    if let Some(c) = card_of(state, a) {
        let _ = write!(out, "## Where {} stands\n\n{}\n", c.id, standing(c));
    }
    live(state, a, ctx, s, &mut out);
    reply_reminder(state, a, s, &mut out);
    out
}

// A resumed turn's reminder of the reply, sent to a planner or foreman when it changed (fac-620).
fn reply_reminder(state: &State, a: &Agent, s: &mut Sections<'_>, out: &mut String) {
    let reminder = reminder(state, a);
    if s.sends("reply", &reminder) {
        out.push_str("\n## The reply\n\n");
        out.push_str(&reminder);
    }
}

fn reminder(state: &State, a: &Agent) -> String {
    let outcomes: Vec<&str> = super::reply::outcomes(state, a).iter().map(|o| o.as_str()).collect();
    let mut out = crate::maintenance::manuals::REMINDER.to_owned();
    if matches!(a.role, Role::Planner | Role::Foreman) {
        out.push(' ');
        out.push_str(crate::maintenance::manuals::FOREMAN_REMINDER);
    }
    let _ =
        writeln!(out, " Your outcomes: {}. `schema` is {}.", outcomes.join(", "), mork_types::reply::SCHEMA_VERSION);
    out
}

// What is said from the state at every turn, a new session's or a resumed one's.
fn live(state: &State, a: &Agent, ctx: Ctx<'_>, s: &mut Sections<'_>, out: &mut String) {
    fixed(state, a, ctx.config, (s, true), out);
    let _ = write!(out, "\n## Context size\n\n{}\n", context_line(a, ctx));
    if let Some(c) = card_of(state, a) {
        out.push_str(&crate::reviews::rounds::under_way(c));
        out.push_str(&crate::reviews::history::for_reviewer(c, a));
    }
    let news = news(state, a);
    out.push_str("\n## ");
    out.push_str(&news);
}

// The sections a planner or foreman is sent when they changed, each with its standing; `plan`: a waiting challenge's
// plan is given with it.
fn fixed(state: &State, a: &Agent, config: &Config, (s, plan): (&mut Sections<'_>, bool), out: &mut String) {
    if planner_foreman(state, a) && s.sends("foreman-specification", FOREMAN_SPECIFICATION) {
        let _ = writeln!(out, "\n{FOREMAN_SPECIFICATION}");
    }
    delegation(state, a, s, out);
    specification(state, a, s, out);
    picks(state, a, config, s, out);
    super::required::section(state, a, (s, plan), out);
}

fn delegation(state: &State, a: &Agent, s: &mut Sections<'_>, out: &mut String) {
    if !matches!(a.role, Role::Planner | Role::Foreman) || planner_foreman(state, a) {
        return;
    }
    let Some(c) = card_of(state, a) else { return };
    let rule = delegation_rule(c);
    let text = format!("\n## Delegation\n\nCurrent level: {}. {rule}\n", c.delegation());
    if s.sends("delegation", &text) {
        out.push_str(&text);
    }
}

/// The specification rule for the card's current delegation level.
pub(crate) fn delegation_rule(c: &Card) -> &'static str {
    if !c.delegated {
        "Settle specification questions with the operator in your flagged hand message."
    } else if c.self_sustained {
        "Raise open specification questions in your flagged hand as with delegation off. The engine gives each ask to the project's orchestrator automatically. The orchestrator answers all given asks, departures included; record each settled rule citing the ask and naming any departure."
    } else {
        "Raise open specification questions in your flagged hand as with delegation off. The engine gives each ask to the project's orchestrator automatically. Name in the ask's text when the answer would depart from the card's words, reverse an earlier decision of the operator's, or need his own words so the orchestrator leaves it to the operator. Record each settled rule citing the ask."
    }
}

fn picks(state: &State, a: &Agent, config: &Config, s: &mut Sections<'_>, out: &mut String) {
    let Some(card) = card_of(state, a).filter(|_| matches!(a.role, Role::Planner | Role::Foreman)) else { return };
    let roles: &[Role] = match card.mode {
        Mode::Full | Mode::Report => &[Role::Researcher],
        Mode::Review => &[Role::Reviewer, Role::Researcher],
        Mode::Light => return,
    };
    let mut text = String::from("\n## Allowed models\n\n");
    for role in roles {
        let _ = writeln!(text, "{}", choices(state, card, config, *role));
    }
    if s.sends("picks", &text) {
        out.push_str(&text);
    }
}

fn choices(state: &State, card: &Card, config: &Config, role: Role) -> String {
    let (_, child) = super::children::child(state, card, role);
    format!("For a {}: {}.", role.as_str(), super::models::pick_options(state, config, &child))
}

// How full an agent's context is, as its CLI last reported it (fac-25): its own call to make
// when it is heavy, `fresh` being the way.
fn context_line(a: &Agent, ctx: Ctx<'_>) -> String {
    if super::models::seat(ctx.config, a).map(|s| s.cli) == Some(mork_types::config::Cli::Codex) {
        return format!(
            "Unknown: {} reports no context size.",
            super::models::cli_name(mork_types::config::Cli::Codex)
        );
    }
    match super::costs::stored_context(ctx.config, a) {
        Some(super::costs::StoredContext { used, window: Some(w), .. }) => {
            format!("{used} of {w} tokens ({}%), at your last turn's end.", used * 100 / w)
        }
        Some(super::costs::StoredContext { used, window: None, .. }) => {
            format!("{used} tokens, at your last turn's end.")
        }
        None => "Not reported yet in this session.".into(),
    }
}

fn card_of<'a>(state: &'a State, a: &Agent) -> Option<&'a Card> {
    places::card_of(state, &a.place).and_then(|id| state.cards.get(&id))
}

// Whom the agent's messages reach, from the router's own table, and that nobody else does, the operator included.
fn recipient_line(state: &State, a: &Agent, ctx: Ctx<'_>) -> String {
    let who = places::Who { key: a.key.clone(), seat: a.seat.clone(), role: a.role, place: a.place.clone() };
    let names = places::recipients(state, &who, ctx);
    let reach = match names.split_last() {
        None => "nobody by name".to_owned(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    };
    let operator = places::hand_only(crate::agents::children::is_child(a));
    let mut chars = operator.chars();
    let operator: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    format!(
        "Your messages reach {reach}. A message to anyone else is not delivered: you are told so, and a request so sent wakes you with the notice. {operator}."
    )
}

fn place_words(state: &State, place: &str) -> String {
    match places::kind(state, place) {
        places::PlaceKind::Main => "main".into(),
        places::PlaceKind::Room(c) if state.cards.get(&c).is_some_and(places::two_rooms) => {
            format!("{c}'s build room, with the operator")
        }
        places::PlaceKind::Room(c) => format!("{c}'s room"),
        places::PlaceKind::Spec(c) => format!("{c}'s spec room, with the operator"),
        places::PlaceKind::Challenge(c) => format!("{c}'s challenge place"),
        places::PlaceKind::Dm(c, r) => format!("a DM with the operator about {c}, as its {}", r.as_str()),
        places::PlaceKind::Child(c, _) => format!("your DM, under {c} in the window"),
        places::PlaceKind::Other(p) => p,
    }
}

fn standing(c: &Card) -> String {
    let head = c.head.as_ref().map_or("none yet", |h| h.as_str());
    format!(
        "Stage: {} · mode: {} · head: {head} · revisions: {}",
        c.stage.as_str(),
        crate::lifecycle::facts::mode_word(c.mode),
        c.revisions
    )
}

fn under_review<'a>(state: &'a State, c: &Card) -> Option<&'a Card> {
    crate::reviews::gate::subject_of(c).and_then(|s| state.cards.get(s)).or_else(|| {
        let pr = c.pr.filter(|_| crate::reviews::gate::entry(c))?;
        crate::queue::approval::owner(state, c.project.as_str(), pr)
    })
}

// The final request includes short retries and corrections. Its saved prefix stays apart from the text to replay.
pub(super) fn after_compaction(state: &State, a: &Agent) -> String {
    let Some(c) = a.needs_specification.then(|| card_of(state, a)).flatten() else { return String::new() };
    let mut text = String::from("# Current card specification\n");
    let notes = card_text(state, c, true, &mut text);
    text.push_str(&later_decisions(&notes));
    if let Some(subject) = under_review(state, c) {
        text.push_str("\n## Under review\n");
        let notes = card_text(state, subject, true, &mut text);
        text.push_str(&later_decisions(&notes));
    }
    text.push('\n');
    text
}

// A blind review keeps the specification's marked decisions, but none of the discussion (fac-194).
fn card(state: &State, c: &Card, blind: bool, out: &mut String) {
    let notes = card_text(state, c, blind, out);
    let _ = write!(out, "\n{}\n", standing(c));
    if let Some(b) = &c.brief {
        let _ = write!(out, "\n## {}\n\n{}\n", brief_heading(c), b.trim());
    }
    out.push_str(&later_decisions(&notes));
}

fn card_text(state: &State, c: &Card, blind: bool, out: &mut String) -> Vec<String> {
    let board = state.board.get(&c.id);
    let (description, acceptance) = board.map_or((&c.description, &c.acceptance), |b| (&b.description, &b.acceptance));
    let title = board.map_or(&c.title, |b| &b.title);
    let (notes, text) =
        if blind { crate::board::notes::apart(state, board, description) } else { (Vec::new(), description.as_str()) };
    let _ = write!(out, "\n## The card\n\n{}: {title}\n\n{}\n", c.id, text);
    carried::accepts(acceptance, out);
    carried::carried(state, c, blind, out);
    notes
}

/// The specification's decisions still standing among these notes, as a blind review keeps them.
pub fn later_decisions(notes: &[String]) -> String {
    standing_decisions(notes, "## The operator's later decisions\n\nPart of the specification, newest first:")
}

/// The decisions still standing among these notes under `heading`, newest first; nothing when none stands.
pub fn standing_decisions(notes: &[String], heading: &str) -> String {
    let decisions = mork_types::decisions::standing(notes);
    if decisions.is_empty() {
        return String::new();
    }
    format!("\n{heading}\n\n{}\n", decisions.iter().map(|d| d.as_str()).collect::<Vec<_>>().join("\n\n"))
}

// A brief its holder's reply settled, nothing left to clarify, was approved by nobody (fac-24).
fn brief_heading(c: &Card) -> &'static str {
    if crate::lifecycle::facts::clear(c) {
        "The brief\n\nThis recap settled it with nothing to clarify: the operator approved no recap."
    } else {
        "The approved brief"
    }
}

// A turn's messages: none of its own but its own request; a review card's reviewer its DM's alone (fac-95); a
// new session no announcement, its start being newer; a role manual's change, its role's seats alone (fac-621).
fn eligible(a: &Agent, m: &Message, addressed_only: bool) -> bool {
    // One its last turn read while it ran is in its session already (fac-389).
    if m.seq < a.seen || a.taken.contains(&m.seq) {
        return false;
    }
    let own = mork_types::state::Sender::Agent(a.key.clone());
    let name = a.key.as_str().rsplit('/').next().unwrap_or_default();
    let manual = crate::maintenance::material::audience(m);
    let addressed = m.to.as_deref().is_some_and(|t| t == name || t == a.key.as_str()) || manual == Some(a.role);
    if (addressed_only && !addressed) || (m.from == own && !(addressed && m.kind == MessageKind::Request)) {
        return false;
    }
    let announced = announce::is(m);
    match a.role {
        Role::Echo => true,
        Role::Planner | Role::Foreman | Role::Reviewer | Role::Researcher | Role::Baseline => {
            !announced || (resumes(a) && manual.is_none_or(|role| role == a.role))
        }
    }
}

// The messages a turn reads: its place's, and a full card's planner, room foreman or foreman's DM also those addressed
// to it in the card's challenge place (fac-633), in their order.
fn read<'a>(state: &'a State, a: &'a Agent) -> Vec<&'a Message> {
    let mut out: Vec<&Message> = state.messages_in(&a.place).filter(|m| eligible(a, m, false)).collect();
    let Some(c) = card_of(state, a).filter(|c| places::two_rooms(c)) else { return out };
    let dm = a.role == Role::Foreman && a.place.as_str() == format!("{}/{}/dm", c.id, Role::Foreman.as_str());
    if !matches!(a.role, Role::Planner | Role::Foreman) || !(dm || a.place.as_str() == places::room_of(&c.id, a.role)) {
        return out;
    }
    let challenge = places::challenge_place(&c.id);
    out.extend(state.messages_in(&challenge).filter(|m| eligible(a, m, true)));
    out.sort_by_key(|m| m.seq);
    out
}

pub(crate) fn unread_request(state: &State, a: &Agent) -> bool {
    read(state, a).into_iter().any(|m| m.kind == MessageKind::Request)
}

// The verb distinguishes requests from responses and updates (fac-18).
fn said(m: &Message) -> String {
    let verb = m.intent.map_or("says", mork_types::reply::Intent::verb);
    let to = match (m.to.as_deref(), m.intent) {
        (Some(t), Some(mork_types::reply::Intent::Update)) => format!(" to {t}"),
        (Some(t), _) => format!(" {t}"),
        (None, _) => String::new(),
    };
    format!("{} {verb}{to}: {}", author(m), m.text)
}

// A message's author as news names it: an orchestrator's answer to an ask the operator gave it says so (fac-160).
fn author(m: &Message) -> String {
    match &m.delegated {
        Some(ask) => format!("{}, on the operator's delegation, answering {}", m.from.label(), ask.as_str()),
        None => m.from.label(),
    }
}

/// The images of the news an agent's next turn reads: the same messages its text does, each path once.
pub(super) fn news_images(state: &State, a: &Agent) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    read(state, a).into_iter().flat_map(|m| m.images.iter()).filter(|i| seen.insert(i.as_str())).cloned().collect()
}

// The news of an agent's place since its last turn, one line per message.
fn news(state: &State, a: &Agent) -> String {
    let mut out = String::from("News since your last turn:\n");
    for m in read(state, a) {
        let Some(line) = news_line(a, m) else { continue };
        let _ = writeln!(out, "{line}");
        if a.missed.contains(&m.seq) {
            out.push_str(MISSED);
        }
    }
    out
}

// What a seat's news says of a message: a foreman reads a specification's approval in one line, the plan being in its
// session already (fac-622), and a card's new notes leave out the seat's own (fac-625).
fn news_line(a: &Agent, m: &Message) -> Option<String> {
    if a.role == Role::Foreman
        && let Some(lead) = announce::approval_lead(m)
    {
        return Some(format!("Mork announcement: {lead}"));
    }
    match announce::heard_by(m, a.key.as_str()) {
        Heard::Whole => line(m),
        Heard::Nothing => None,
        Heard::Others(text) => line(&Message { text, ..m.clone() }),
    }
}

// A message delivered into a turn that ended before reading it may still have reached its session (fac-389).
const MISSED: &str = "(Delivered while your last turn ran, which ended before it read it: it may already be in your session. Act on it once.)\n";

const QUESTION_REMINDER: &str = "The operator's message contains a question. Answer it in plain words, the answer first, from the evidence. A question is a probe, never an instruction and never a hint of what he wants: change nothing because of it, in your files or in what you proposed. If your own answer turns up a reason to change, say the reason and then the change. Act only on the parts that tell you to do something.";

/// A message delivered into its recipient's running turn (fac-389), as its news says it.
pub fn delivered(m: &Message) -> String {
    let line = line(m).unwrap_or_else(|| m.text.clone());
    format!("A new message arrived while you work. Take it into account in this turn:\n{line}")
}

fn line(m: &Message) -> Option<String> {
    let n = m.number.map(|n| format!("#{n} ")).unwrap_or_default();
    let to = m.to.as_deref().map(|t| format!(" (to {t})")).unwrap_or_default();
    let mut out = match m.kind {
        MessageKind::Hand => format!("{n}{}'s hand: {}\n{}", m.from.label(), m.headline, m.text),
        MessageKind::Engine if announce::is(m) => format!("Mork announcement: {}", m.text),
        MessageKind::Request | MessageKind::Update if m.intent.is_some() => format!("{n}{}", said(m)),
        MessageKind::Request | MessageKind::Update if m.from != mork_types::state::Sender::Operator => {
            format!("{n}{}{to}: {}", author(m), m.text)
        }
        MessageKind::Request | MessageKind::Update | MessageKind::Engine => format!("{n}{}: {}", author(m), m.text),
        // None is in a place's messages anymore.
        MessageKind::Feedback => return None,
    };
    // The answer's chosen option is typed apart from its words (fac-696).
    if let Some(n) = m.choice {
        out.push_str(&format!("\n(It chose option {n} of your ask.)"));
    }
    if m.from == mork_types::state::Sender::Operator && m.text.contains('?') {
        out.push('\n');
        out.push_str(QUESTION_REMINDER);
    }
    Some(out)
}
