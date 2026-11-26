//! A place's conversation (5.7.5): its messages, the page of earlier ones, and the turns between them. Turn positions
//! come from existing jobs and messages, without adding a record (fac-138).

use std::collections::BTreeSet;

use mork_types::ask::{AskKind, Color};
use mork_types::card::Mode;
use mork_types::chat::{ChatTurn, MessagesView};
use mork_types::job::JobKind;
use mork_types::reply::Role;
use mork_types::seen::{ReadPoint, TurnView};
use mork_types::state::{Message, Sender};
use mork_types::view::{Author, HistoryPage, ImageView, Line, MessageView};
use mork_types::{AgentKey, PlaceId};

use crate::state::State;

/// The largest message page a conversation view publishes.
pub const MESSAGES_MAX: usize = 5000;

/// A place's conversation alone, as `place` shows it (fac-280): nothing of its card, agents or work.
pub fn messages(state: &State, place: &str, tail: Option<u64>) -> MessagesView {
    let (messages, earlier, pinned) = conversation(state, place, tail);
    MessagesView { place: PlaceId::new(place), revision: state.revision, messages, earlier, pinned }
}

/// Up to `count` of a place's messages just before the one numbered `before`, oldest first: a
/// page of what a view's tail left out (fac-42).
pub fn history(state: &State, place: &str, before: u64, count: u64) -> HistoryPage {
    let all: Vec<&Message> = state.messages_in(place).collect();
    let open = open_asks(state, place, &all);
    let rf = review_foreman(state, place);
    let end = all.iter().position(|m| m.seq >= before).unwrap_or(all.len());
    let start = end.saturating_sub(usize::try_from(count).unwrap_or(usize::MAX));
    let messages = all[start..end].iter().map(|m| shown(m, &open, rf.as_ref())).collect();
    HistoryPage { place: PlaceId::new(place), messages, earlier: start, turns: Vec::new() }
}

// Its latest `tail` messages, never more than any line can carry; how many came before; and
// those of them whose ask is open, which the tail never hides (fac-42).
pub(super) fn conversation(
    state: &State,
    place: &str,
    tail: Option<u64>,
) -> (Vec<MessageView>, usize, Vec<MessageView>) {
    let all: Vec<&Message> = state.messages_in(place).collect();
    let open = open_asks(state, place, &all);
    let tail = tail.map_or(MESSAGES_MAX, |t| usize::try_from(t).unwrap_or(usize::MAX).min(MESSAGES_MAX));
    let earlier = all.len().saturating_sub(tail);
    let rf = review_foreman(state, place);
    let pinned =
        all[..earlier].iter().filter(|m| open.contains(&m.seq)).map(|m| shown(m, &open, rf.as_ref())).collect();
    (all[earlier..].iter().map(|m| shown(m, &open, rf.as_ref())).collect(), earlier, pinned)
}

// A review card's foreman reads `review foreman` in its lines, as on the rail (fac-691).
fn review_foreman(state: &State, place: &str) -> Option<AgentKey> {
    let card = crate::messages::places::card_of(state, place)?;
    (state.cards.get(&card)?.mode == Mode::Review).then(|| AgentKey::new(format!("{card}/foreman")))
}

fn shown(m: &Message, open: &BTreeSet<u64>, review_foreman: Option<&AgentKey>) -> MessageView {
    let mut v = MessageView { ask_open: open.contains(&m.seq), ..message(m) };
    if let (Sender::Agent(k), Author::Agent { label, .. }) = (&m.from, &mut v.from)
        && Some(k) == review_foreman
    {
        "review foreman".clone_into(label);
    }
    v
}

// An `answer` ask is the hand that raised it, which later replies leave open (fac-271); a block, a
// recap's or a report's gate is its agent's latest hand's, each next reply closing or replacing it.
pub(super) fn open_asks(state: &State, place: &str, all: &[&Message]) -> BTreeSet<u64> {
    let card = crate::messages::places::card_of(state, place);
    let hands: Vec<&Message> = all.iter().copied().filter(|m| m.hand.is_some()).collect();
    state
        .asks
        .values()
        .filter(|a| a.open && (a.place == place || a.card.is_some() && a.card == card))
        .filter(|a| matches!(a.kind, AskKind::Answer | AskKind::Blocked | AskKind::ApproveBrief | AskKind::Done))
        .filter_map(|a| source_hand(state, a, &hands))
        .collect()
}

/// The seq of the hand among `hands` that raised ask `a`, open or closed.
pub(super) fn source_hand(state: &State, a: &mork_types::ask::Ask, hands: &[&Message]) -> Option<u64> {
    let key = owner(state, a)?;
    let mine = hands.iter().rev().filter(|m| matches!(&m.from, Sender::Agent(k) if *k == key));
    let raised = |m: &&&Message| {
        m.hand.as_ref().is_some_and(|h| {
            if a.kind == AskKind::Answer {
                // A hand after the ask's raising can't be its source; the red fallback is a red hand
                // with an empty `ask`, not withdrawing.
                let red = h.ask.is_empty() && h.color == Color::Red && !h.withdraw_ask && h.headline == a.text;
                m.time <= a.raised_ms && (h.ask_words().trim() == a.text || red)
            } else {
                !h.ask.trim().is_empty()
            }
        })
    };
    let found = if a.kind == AskKind::Answer { mine.into_iter().find(raised) } else { mine.take(1).find(raised) };
    found.map(|m| m.seq)
}

// A report's `done` ask names no agent: its foreman raised it with its report (4.4.5). A review
// card's names none either, and carries no hand's ask.
fn owner(state: &State, a: &mork_types::ask::Ask) -> Option<AgentKey> {
    if let Some(key) = &a.agent {
        return Some(key.clone());
    }
    let card = state.cards.get(a.card.as_ref()?)?;
    (a.kind == AskKind::Done && card.mode == Mode::Report).then(|| AgentKey::new(format!("{}/foreman", card.id)))
}
// An engine message to an agent is a marker whose text unfolds under it (fac-384); one to the operator shows whole (5.7.5).
pub(super) fn message(m: &Message) -> MessageView {
    let (from, kind) = match &m.from {
        Sender::Operator => (Author::Operator, Line::Operator),
        Sender::Orchestrator(p) => (Author::Orchestrator { project: p.to_string() }, Line::Agent),
        Sender::Pa => (Author::Pa, Line::Agent),
        Sender::Engine if m.to.as_deref().is_some_and(|t| t != "operator") => (Author::Engine, Line::Marker),
        Sender::Engine if crate::maintenance::material::is_announcement(m) => (Author::Engine, Line::Marker),
        Sender::Engine => (Author::Engine, Line::Engine),
        Sender::Agent(key) => {
            // The role the message was posted with (M5.26; version 4 filled it in, M5.29); one
            // fac-119 removed reads as none, and draws plain.
            let role = m.role.unwrap_or(Role::Echo);
            let label = key.as_str().split('/').skip(1).collect::<Vec<_>>().join("/");
            (Author::Agent { label, role }, Line::Agent)
        }
    };
    MessageView {
        seq: m.seq,
        time_ms: m.time,
        from,
        to: m.to.clone(),
        kind,
        headline: m.headline.clone(),
        text: m.text.clone(),
        cut: false,
        number: m.number,
        hand: m.hand.clone(),
        findings: m.findings.clone(),
        images: m.images.iter().map(|p| image(p)).collect(),
        ask_open: false,
        choice: m.choice,
    }
}

// A job's published copy, `…/jobs/<job>/published/<file>`, is served at `/images/<job>/<file>` (fac-696); a pasted
// image has no URL.
fn image(path: &str) -> ImageView {
    let parts: Vec<&str> = path.rsplit('/').take(4).collect();
    let url = match parts.as_slice() {
        [file, "published", job, "jobs"] if !file.is_empty() && !job.is_empty() => {
            Some(format!("/images/{}/{}", encoded(job), encoded(file)))
        }
        _ => None,
    };
    ImageView { path: path.to_owned(), url }
}

fn encoded(segment: &str) -> String {
    let keep = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
    segment.bytes().map(|b| if keep(b) { char::from(b).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The jobs belonging between this page's messages, including a turn before its first reply.
pub fn turns(state: &State, place: &str, messages: &[MessageView], before: Option<u64>) -> Vec<ChatTurn> {
    let all: Vec<_> = state.messages_in(place).collect();
    let first = messages.first().map_or(0, |m| m.seq);
    let floor = all.iter().take_while(|m| m.seq < first).last().map_or(0, |m| m.seq);
    // The earliest time from the floor on: a turn made before it and ended before the page can't
    // follow the floor, so it isn't matched against every message (fac-336).
    let since = all.iter().filter(|m| floor > 0 && m.seq >= floor).map(|m| m.time).min().unwrap_or(i64::MIN);
    let mut jobs: Vec<_> = state
        .jobs
        .values()
        .filter(|j| j.kind == JobKind::Turn && place_of(state, &j.owner).as_deref() == Some(place))
        .collect();
    jobs.sort_by_key(|j| (j.spec.created_ms, &j.id));
    jobs.into_iter()
        .filter_map(|j| {
            let overlaps = j.settled_ms.is_none_or(|end| messages.first().is_none_or(|m| end >= m.time_ms));
            if j.spec.created_ms < since && !overlaps {
                return None;
            }
            let after_seq = all
                .iter()
                .filter(|m| m.time < j.spec.created_ms || (m.time == j.spec.created_ms && m.seq < j.seen.unwrap_or(0)))
                .map(|m| m.seq)
                .max()
                .unwrap_or(0);
            ((after_seq >= floor || overlaps) && before.is_none_or(|b| after_seq < b)).then(|| ChatTurn {
                agent: j.owner.clone(),
                after_seq,
                running: j.settled.is_none(),
                step_count: 0,
                turn: TurnView {
                    job: j.id.clone(),
                    read: j.read.iter().flatten().map(|&seq| ReadPoint { seq, step: 0 }).collect(),
                    ..TurnView::default()
                },
            })
        })
        .collect()
}

fn place_of(state: &State, key: &mork_types::AgentKey) -> Option<String> {
    if let Some(a) = state.agents.get(key) {
        return Some(a.place.to_string());
    }
    // A numbered child's key keeps its turns in its DM once the seat is gone.
    let (parent, name) = key.as_str().rsplit_once('/')?;
    if name == "dm" {
        Some(key.to_string())
    } else if name == Role::Planner.as_str() {
        Some(crate::messages::places::spec_room(&mork_types::CardId::new(parent)))
    } else if crate::agents::children::role_of(name).is_some() || crate::agents::children::builder_dm_name(name) {
        Some(format!("{key}/dm"))
    } else {
        Some(parent.to_owned())
    }
}
