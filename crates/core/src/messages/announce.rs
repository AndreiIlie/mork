//! Mork announcements (fac-14): the engine's lines to everyone in a place, or a role manual's change to that role's
//! seats alone (fac-621); they wake nobody and reach each agent addressed with its next turn's news.

use std::collections::BTreeMap;

use mork_types::CardId;
use mork_types::card::{BoardCard, Card};
use mork_types::reply::Role;
use mork_types::state::{Message, MessageKind, Sender};
use serde_json::json;

use super::places;
use crate::board::notes::apart;
use crate::post::Poster;

/// Whether a message is a Mork announcement: the engine's, to no one in particular or to one role's seats.
pub(crate) fn is(m: &Message) -> bool {
    m.from == Sender::Engine && m.kind == MessageKind::Engine && m.to.as_deref().is_none_or(|t| role_of(t).is_some())
}

/// The role an announcement is addressed to; none for one to every seat.
pub(crate) fn addressed_role(m: &Message) -> Option<Role> {
    m.to.as_deref().and_then(role_of)
}

fn role_of(to: &str) -> Option<Role> {
    Role::ALL.into_iter().find(|r| r.as_str() == to)
}

/// Posts an announcement in a place: no one is woken.
pub(crate) fn post(p: &mut Poster<'_>, place: &str, role: Option<&str>, headline: &str, text: &str) {
    p.post(place, Sender::Engine, role, MessageKind::Engine, headline, text);
}

// A card's announcements reach its rooms, its spec room too (fac-633), and each DM of it with an agent, its seats'
// children's included (fac-94).
fn card_places(p: &Poster<'_>, card: &CardId) -> Vec<String> {
    let mut out = vec![card.as_str().to_owned()];
    if p.card(card).is_some_and(|c| places::two_rooms(&c)) {
        out.push(places::spec_room(card));
    }
    for r in places::CARD_ROLES {
        let dm = format!("{card}/{}/dm", r.as_str());
        if p.agent(&mork_types::AgentKey::new(dm.clone())).is_some() {
            out.push(dm);
        }
    }
    out.extend(crate::agents::children::of_card(p.state, card).into_iter().map(|w| w.place.to_string()));
    out
}

/// Posts an announcement in a card's rooms and in each DM of it.
pub(crate) fn to_card(p: &mut Poster<'_>, card: &CardId, headline: &str, text: &str) {
    for place in card_places(p, card) {
        post(p, &place, None, headline, text);
    }
}

/// A live run's card, its own or one it carries, whose words changed since its agents had them is announced whole in
/// its places, never as an approval; compared with the run's copy, so a change made while the engine was down counts.
pub(crate) fn boards_read(p: &mut Poster<'_>, cards: &[BoardCard]) {
    for b in cards {
        let Some(mut c) = holder(p, &b.id) else { continue };
        let Some((title, description, acceptance)) = saved(&mut c, &b.id) else { continue };
        if (&*title, &*description, &*acceptance) == (&b.title, &b.description, &b.acceptance) {
            continue;
        }
        let (old_notes, old_body) = apart(p.state, Some(b), description);
        let (notes_all, body) = apart(p.state, Some(b), &b.description);
        let changed = (&*title, &old_body, &*acceptance) != (&b.title, &body, &b.acceptance);
        let added = added_notes(&old_notes, &notes_all);
        title.clone_from(&b.title);
        description.clone_from(&b.description);
        acceptance.clone_from(&b.acceptance);
        let room = c.id.clone();
        if changed {
            to_card(p, &room, &format!("{}'s card changed", b.id), &card_text(&room, b, &body));
        }
        if !added.is_empty() {
            noted(p, &room, &b.id, &added);
            decisions(p, &c, &b.id, &notes_all, &added);
        }
        p.put_card(c);
        let row = json!({"card": b.id, "text": changed, "notes": added.len()});
        p.journal("card.announced", Some(room.as_str()), row);
    }
}

// The live run whose places hear of a card's edits: its own, or the one carrying it.
fn holder(p: &Poster<'_>, id: &CardId) -> Option<Card> {
    let live = |c: &Card| c.stage.is_live();
    p.card(id).filter(live).or_else(|| p.card(&crate::lifecycle::carried::carrying(p.state, id, live)?.id))
}

// The words a run last saved of one of its cards: what its agents were last given.
fn saved<'c>(c: &'c mut Card, id: &CardId) -> Option<(&'c mut String, &'c mut String, &'c mut Vec<String>)> {
    if c.id == *id {
        return Some((&mut c.title, &mut c.description, &mut c.acceptance));
    }
    c.carries.iter_mut().find(|k| k.id == *id).map(|k| (&mut k.title, &mut k.description, &mut k.acceptance))
}

// The notes of `now` the old ones don't account for, newest first: identical notes (the same sender
// and day) count one by one, the oldest ones being the old ones.
fn added_notes(old: &[String], now: &[String]) -> Vec<String> {
    let mut kept: BTreeMap<&str, usize> = BTreeMap::new();
    for n in old {
        *kept.entry(n).or_default() += 1;
    }
    let mut added: Vec<String> = now
        .iter()
        .rev()
        .filter(|n| match kept.get_mut(n.as_str()) {
            Some(left) if *left > 0 => {
                *left -= 1;
                false
            }
            _ => true,
        })
        .cloned()
        .collect();
    added.reverse();
    added
}

// A card's new notes, to its run's room and DMs: discussion, never its announcement. Each message records where its
// entries lie, so a seat's news can leave out its own (fac-625).
fn noted(p: &mut Poster<'_>, room: &CardId, card: &CardId, notes: &[String]) {
    let mut text =
        format!("New notes on {card}'s card, newest first: its discussion, neither its text nor an approval.\n");
    let mut entries = Vec::with_capacity(notes.len());
    for note in notes {
        text.push('\n');
        let start = text.len();
        text.push_str(note);
        entries.push(start..text.len());
    }
    for place in card_places(p, room) {
        p.post(
            &place,
            Sender::Engine,
            Some("room"),
            MessageKind::Update,
            &format!("{card}'s card has new notes"),
            &text,
        );
        if let Some(m) = p.last_message() {
            m.entries = Some(entries.clone().into());
        }
    }
}

/// What a seat reads of a message that lists a card's new notes.
pub(crate) enum Heard {
    /// The message as it was posted.
    Whole,
    /// Its text with the entries the seat wrote left out.
    Others(String),
    /// Every entry is the seat's own: nothing is left to read.
    Nothing,
}

/// A card's new notes as the seat `me` reads them: the entries of others, whole and in order, never its own (fac-625).
/// A message with no recorded entries, an older one among them, is heard whole.
pub(crate) fn heard_by(m: &Message, me: &str) -> Heard {
    let Some(entries) = &m.entries else { return Heard::Whole };
    let parts: Option<Vec<&str>> = entries.iter().map(|r| m.text.get(r.clone())).collect();
    let (Some(parts), Some(head)) = (parts, entries.first().and_then(|r| m.text.get(..r.start))) else {
        return Heard::Whole;
    };
    let kept: Vec<&str> =
        parts.iter().copied().filter(|e| e.split_once(' ').is_none_or(|(who, _)| who != me)).collect();
    match kept.len() {
        0 => Heard::Nothing,
        n if n == parts.len() => Heard::Whole,
        _ => Heard::Others(format!("{head}{}", kept.join("\n"))),
    }
}

// What reaches the run's review card: the new decisions on one of its cards that still stand, and the words of those
// withdrawn.
fn decisions(p: &mut Poster<'_>, card: &Card, about: &CardId, all: &[String], added: &[String]) {
    use mork_types::decisions::{is_withdrawal, standing};
    let standing = standing(all);
    let new: Vec<_> = added.iter().filter(|n| standing.contains(n)).cloned().collect();
    let gone: Vec<_> = added.iter().filter(|n| is_withdrawal(n)).cloned().collect();
    if new.is_empty() && gone.is_empty() {
        return;
    }
    let review = card
        .pr
        .and_then(|pr| p.card(&crate::reviews::cards::card_id(card.project.as_str(), pr)))
        .filter(|c| c.stage.is_live() && crate::reviews::gate::entry(c))
        .or_else(|| crate::reviews::gate::card_for(p, card));
    let Some(review) = review.filter(|r| r.id != card.id) else { return };
    let mut parts = Vec::new();
    if !new.is_empty() {
        parts.push(format!(
            "The operator's later decisions on {about}, part of its specification, newest first:\n\n{}",
            new.join("\n\n")
        ));
    }
    if !gone.is_empty() {
        parts.push(format!(
            "Withdrawn on {about}: these decisions are no longer part of its specification, whatever you were given before:\n\n{}",
            gone.join("\n\n")
        ));
    }
    let text = parts.join("\n\n");
    let mut places = vec![review.id.to_string()];
    places.extend(
        crate::reviews::gate::agents_of(p, &review.id).iter().filter_map(|k| p.agent(k).map(|a| a.place.to_string())),
    );
    places.sort();
    places.dedup();
    for place in places {
        p.post(
            &place,
            Sender::Engine,
            Some("room"),
            MessageKind::Update,
            &format!("The operator's later decisions on {about}"),
            &text,
        );
    }
}

fn card_text(room: &CardId, b: &BoardCard, body: &str) -> String {
    let carried = if *room == b.id { String::new() } else { format!(", which {room}'s run carries,") };
    let mut text = format!(
        "{}'s card{carried} changed on its board. This is its whole text now, replacing the one you were given: context for its work, not an approval. Its approved specification, if it has one, is still its approved brief.\n\n{}: {}\n\n{}\n",
        b.id,
        b.id,
        b.title,
        body.trim()
    );
    if !b.acceptance.is_empty() {
        text.push_str("\nAcceptance:\n");
        for line in &b.acceptance {
            text.push_str(&format!("- {line}\n"));
        }
    }
    text
}

const APPROVED_NOW: &str = " This brief is its approved specification now:";
const APPROVED_HEADLINE: &str = "'s specification is approved";

/// Announces the approved specification whole to its places, naming who approved it.
pub(crate) fn brief_approved(p: &mut Poster<'_>, card: &CardId, brief: &str) {
    let Some(approval) = p.card(card).and_then(|c| c.approval) else { return };
    let by = crate::lifecycle::brief::approval_label(&approval.by);
    let text = format!("{by} approved {card}'s specification.{APPROVED_NOW}\n\n{}", brief.trim());
    to_card(p, card, &format!("{card}{APPROVED_HEADLINE}"), &text);
}

/// The approval sentence from the card's room, where the foreman already holds the plan (fac-622).
pub(crate) fn approval_lead(m: &Message) -> Option<&str> {
    if !is(m) || m.headline.strip_suffix(APPROVED_HEADLINE) != Some(m.place.as_str()) {
        return None;
    }
    m.text.split_once(APPROVED_NOW).map(|(lead, _)| lead)
}
