// The Overview's lines (fac-684, fac-691): the card's rooms' messages for Andrei, without the engine's boilerplate, each
// answered ask carrying the line that answered it.

use std::collections::{BTreeMap, BTreeSet};

use factory_types::ask::AskKind;
use factory_types::card::{Card, Mode};
use factory_types::reply::{Intent, Role};
use factory_types::review::Review;
use factory_types::state::{Message, MessageKind, Sender};
use factory_types::view::{Author, Line, MessageView, OverviewLine};

use super::conversation::{message, open_asks, source_hand};
use crate::messages::{announce, places};
use crate::queue::members::VERIFICATION_STARTED;
use crate::state::State;

// One room the Overview reads: a one-room full card's own room reads Spec until `spec_until`, its build's start.
struct Room {
    place: String,
    label: &'static str,
    first_seq: u64,
    review: bool,
    spec_until: Option<i64>,
}

// An answered ask's hand, by its seq, and the message that answered it.
type Answers<'a> = BTreeMap<u64, &'a Message>;

/// The card's Overview lines, oldest first, with their times and seqs; `build_ms` is its first Build stage's start.
pub(super) fn collect_lines(
    state: &State,
    card: &Card,
    review: Option<(&Card, &Review)>,
    build_ms: Option<i64>,
) -> Vec<(i64, u64, OverviewLine)> {
    let rooms = rooms(state, card, review, build_ms);
    let answers = answers(state, card, &rooms);
    let answering: BTreeSet<u64> = answers.values().map(|m| m.seq).collect();
    let mut out = Vec::new();
    for room in &rooms {
        gather(state, card, room, (&answers, &answering), &mut out);
    }
    out.sort_by_key(|(time_ms, seq, _)| (*time_ms, *seq));
    out
}

fn rooms(state: &State, card: &Card, review: Option<(&Card, &Review)>, build_ms: Option<i64>) -> Vec<Room> {
    let (label, is_review) = match card.mode {
        Mode::Review => ("Review", true),
        Mode::Report => ("Report", false),
        Mode::Full | Mode::Light => ("Build", false),
    };
    let one_room = card.mode == Mode::Full && !places::two_rooms(card);
    let spec_until = one_room.then(|| build_ms.unwrap_or(i64::MAX));
    let own = card.id.to_string();
    let mut out = vec![Room { place: own, label, first_seq: card.first_seq, review: is_review, spec_until }];
    let spec = places::spec_room(&card.id);
    if places::two_rooms(card) && state.places.contains_key(spec.as_str()) {
        out.push(Room { place: spec, label: "Spec", first_seq: card.first_seq, review: false, spec_until: None });
    }
    if card.mode != Mode::Review
        && let Some((rc, _)) = review
    {
        let first_seq = card.first_seq.max(rc.first_seq);
        out.push(Room { place: rc.id.to_string(), label: "Review", first_seq, review: true, spec_until: None });
    }
    out
}

// Andrei's or the orchestrator's message posted as the ask closed answered it (fac-691).
fn answers<'a>(state: &'a State, card: &Card, rooms: &[Room]) -> Answers<'a> {
    let all: Vec<&Message> = rooms.iter().flat_map(|r| state.messages_in(&r.place)).collect();
    let hands: Vec<&Message> = all.iter().copied().filter(|m| m.hand.is_some()).collect();
    let mut out = Answers::new();
    let answered =
        state.asks.values().filter(|a| a.kind == AskKind::Answer && !a.open && a.card.as_ref() == Some(&card.id));
    for a in answered {
        let Some(at) = a.answered_ms else { continue };
        let by = |m: &&&Message| m.time == at && (m.from == Sender::Andrei || m.from.is_orchestrator());
        if let (Some(hand), Some(answer)) = (source_hand(state, a, &hands), all.iter().find(by)) {
            out.insert(hand, answer);
        }
    }
    out
}

fn gather(
    state: &State,
    card: &Card,
    room: &Room,
    (answers, answering): (&Answers<'_>, &BTreeSet<u64>),
    out: &mut Vec<(i64, u64, OverviewLine)>,
) {
    let all: Vec<&Message> = state.messages_in(&room.place).collect();
    let open = open_asks(state, &room.place, &all);
    for m in all.into_iter().filter(|m| m.seq >= room.first_seq && !answering.contains(&m.seq)) {
        let mut view = message(m);
        view.ask_open = open.contains(&m.seq);
        if !for_andrei(&view) || is_echo(state, m, &view, room.review) || boilerplate(card, m) {
            continue;
        }
        if let Some(lead) = announce::approval_lead(m) {
            view.headline = lead.trim().to_owned();
            view.text = String::new();
        }
        if room.review
            && is_review_foreman(m)
            && let Author::Agent { label, .. } = &mut view.from
        {
            "review foreman".clone_into(label);
        }
        let label = match room.spec_until {
            Some(t) if m.time < t => "Spec",
            _ => room.label,
        };
        let answer = answers.get(&m.seq).map(|a| message(a));
        out.push((m.time, m.seq, OverviewLine { room: label.to_owned(), message: view, answer }));
    }
}

fn for_andrei(v: &MessageView) -> bool {
    matches!(v.kind, Line::Andrei | Line::Engine)
        || (v.kind == Line::Agent && (v.hand.is_some() || v.to.is_none() || v.to.as_deref() == Some("andrei")))
}

// The engine's "verification started" and a seat's deliverables line say what the spine and live rows show; the
// approval's copy outside the card's own room repeats its line there.
fn boilerplate(card: &Card, m: &Message) -> bool {
    let verification = m.from == Sender::Engine && m.headline == VERIFICATION_STARTED;
    let deliverables = matches!(m.from, Sender::Agent(_))
        && m.kind == MessageKind::Update
        && m.intent == Some(Intent::Update)
        && m.to.is_none()
        && m.hand.is_none();
    let copy = announce::is_approval(m, &card.id) && m.place != card.id.as_str();
    verification || deliverables || copy
}

// Review verdicts repeat the round milestone; engine notices of open engine asks repeat the Raised Hand card.
fn is_echo(state: &State, m: &Message, v: &MessageView, is_review: bool) -> bool {
    if is_review && is_review_foreman(m) && v.hand.as_ref().is_some_and(|h| h.ask.trim().is_empty()) {
        return true;
    }
    v.kind == Line::Engine
        && state.asks.values().any(|a| a.open && a.agent.is_none() && a.place == m.place && a.raised_ms == m.time)
}

fn is_review_foreman(m: &Message) -> bool {
    m.role == Some(Role::Foreman) || matches!(&m.from, Sender::Agent(k) if k.as_str().ends_with("/foreman"))
}
