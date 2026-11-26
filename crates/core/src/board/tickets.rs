//! A ticketed project's card tickets (fac-290): read from its notes, they name its PR, its branch and
//! its description.

use mork_types::card::Card;
use mork_types::config::Config;
use mork_types::{CardId, ProjectId};

use crate::state::State;

/// Whether a project has `[projects.<name>.tickets]` configured (fac-721).
pub fn has_tickets(config: &Config, project: &ProjectId) -> bool {
    config.projects.get(project).is_some_and(|p| p.tickets.is_some())
}

// The numbers a note lists after `Azure DevOps` (`4123, 4124`, or `User Story 4650 under epic 4607`:
// the epic is no ticket of the card).
fn listed(rest: &str) -> Vec<u64> {
    let mut rest = rest.strip_prefix("User Story ").unwrap_or(rest);
    let mut out = Vec::new();
    loop {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let Some(n) = rest[..digits].parse().ok() else { return out };
        out.push(n);
        match rest[digits..].strip_prefix(',') {
            Some(more) => rest = more.trim_start(),
            None => return out,
        }
    }
}

// A note's first line's text after its author and date: a decision's starts with its mark.
fn note_text(first: &str) -> Option<&str> {
    let (_, rest) = first.split_once(' ').filter(|_| super::notes::is_note(first))?;
    Some(rest[6..].trim())
}

fn ticket_note(n: &str, label: &str) -> Vec<u64> {
    let text = n.lines().next().and_then(note_text);
    text.and_then(|t| t.strip_prefix(label)?.strip_prefix(' ')).map(listed).unwrap_or_default()
}

/// A card's ticket numbers (fac-351): those of its newest ticket note, in its order, then each carried
/// card's (fac-563), then those its newest `PR wording` note adds, in its order; none when the project has no `tickets` table.
pub fn of(state: &State, c: &Card, config: &Config) -> Vec<u64> {
    listed_of(state, c, config, true)
}

/// As [`of`], leaving out the cards the run carries: its branch is named from its own card alone.
pub fn own(state: &State, c: &Card, config: &Config) -> Vec<u64> {
    listed_of(state, c, config, false)
}

fn listed_of(state: &State, c: &Card, config: &Config, carried: bool) -> Vec<u64> {
    let Some(tickets) = config.projects.get(&c.project).and_then(|p| p.tickets.as_ref()) else {
        return Vec::new();
    };
    let label = tickets.label();
    let mut out = noted(state, &c.id, &c.description, label);
    let carried = c.carries.iter().filter(|_| carried).flat_map(|k| noted(state, &k.id, &k.description, label));
    let added = wording(state, c).map(|w| w.1).unwrap_or_default().into_iter().map(|(n, _)| n);
    for n in carried.chain(added) {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

// The numbers of a card's newest ticket note, read from its board, else from the words its run saved.
fn noted(state: &State, id: &CardId, saved: &str, label: &str) -> Vec<u64> {
    let board = state.board.get(id);
    let (notes, _) = crate::board::notes::apart(state, board, board.map_or(saved, |b| b.description.as_str()));
    notes.iter().map(|n| ticket_note(n, label)).find(|t| !t.is_empty()).unwrap_or_default()
}

fn body(n: &str) -> Option<std::str::Lines<'_>> {
    let mut lines = n.lines();
    (note_text(lines.next()?)? == "PR wording").then_some(lines)
}

fn wording(state: &State, c: &Card) -> Option<(String, Vec<(u64, String)>)> {
    let description = state.board.get(&c.id).map_or(&c.description, |b| &b.description);
    let (notes, _) = crate::board::notes::apart(state, state.board.get(&c.id), description);
    let mut lines = notes.iter().find_map(|n| body(n))?;
    let title = lines.next()?.trim().strip_prefix("Title:")?.trim().to_owned();
    let each = lines.filter_map(|l| {
        let (n, note) = l.split_once(':')?;
        Some((n.trim().trim_start_matches('#').parse().ok()?, note.trim().to_owned()))
    });
    Some((title, each.collect()))
}

/// A card's PR title's wording: the foreman's summary from its `PR wording` note, whole; none when
/// it has no ticket or no such note.
pub fn summary(state: &State, c: &Card, config: &Config) -> Option<String> {
    wording(state, c).map(|w| w.0).filter(|t| !t.is_empty() && !of(state, c, config).is_empty())
}

/// The request to add a card's PR wording before publication.
pub(crate) fn wording_request(skill: &str) -> String {
    format!(
        "Its PR opens from the card's `PR wording` note: add it now with `card.change --note`, its first line `PR wording`, then `Title: <a summary of under 10 words>` and one `<ticket number>: <a note of about 15 words>` line for every ticket (the {skill} skill)."
    )
}

/// Why a card's PR can't be published yet: a ticketed project's card with tickets needs a `PR wording`
/// note with a summary and a note for every ticket (fac-296); none when it is complete or no ticket's.
pub fn unworded(state: &State, c: &Card, config: &Config) -> Option<String> {
    let tickets = of(state, c, config);
    if tickets.is_empty() {
        return None;
    }
    let (title, notes) = wording(state, c).unwrap_or_default();
    let lacking: Vec<String> = tickets
        .iter()
        .filter(|n| !notes.iter().any(|(m, note)| m == *n && !note.is_empty()))
        .map(|n| format!("#{n}"))
        .collect();
    if !title.is_empty() && lacking.is_empty() {
        return None;
    }
    Some("its PR has no complete wording: add a card note with `PR wording` as its first line, then `Title: <summary>` and one `<ticket number>: <note>` line for every ticket, then /action.retry".to_owned())
}

/// A card's PR description: one line a ticket, its page and the foreman's note of it when its
/// `PR wording` note has one; none for a card with no ticket.
pub fn description(state: &State, c: &Card, config: &Config) -> String {
    let Some(page) = config.projects.get(&c.project).and_then(|p| p.tickets.as_ref()).map(|t| t.page()) else {
        return String::new();
    };
    let notes = wording(state, c).map(|w| w.1).unwrap_or_default();
    let line = |n: &u64| match notes.iter().find(|(m, _)| m == n) {
        Some((_, note)) => format!("[{n}]({page}{n}): {note}"),
        None => format!("{page}{n}"),
    };
    of(state, c, config).iter().map(line).collect::<Vec<_>>().join("\n")
}

/// The branch a card starts on (fac-111): its id, a ticketed project's card with a ticket starting with
/// the first one's number and its title's words, each the push's internals check would refuse left out (fac-394).
pub fn branch(state: &State, c: &Card, config: &Config) -> String {
    let Some(first) = own(state, c, config).first().copied() else { return c.id.as_str().to_lowercase() };
    let title = crate::queue::pr::title_words(state, c, config).to_lowercase();
    let mut name = first.to_string();
    for w in title.split(|ch: char| !ch.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        let longer = format!("{name}-{w}");
        if refused(&config.internals, &longer).is_none() {
            name = longer;
        }
    }
    name
}

/// What the push's internals check refuses in a branch's name; a slug holds no path.
pub fn refused(internals: &mork_types::config::Internals, branch: &str) -> Option<String> {
    crate::internals::internals(internals, "", branch)
}
