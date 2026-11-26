use std::collections::BTreeMap;

use mork_types::card::{BoardCard, Card, Landing, Stage};
use mork_types::view::{AgentLight, BoardGroup, BoardRow, BoardView, LiveMarks, MergeQueue};
use mork_types::{CardId, ProjectId};

use super::findings::{folded_parent, folded_review};
use super::order::{by_number, live_cards};
use super::outside::Outside;
use super::status::{agent_light, card_light, stage_word};
use crate::decide::Ctx;
use crate::state::State;

/// The label that puts a card in an area: `area:<name>`.
pub const AREA: &str = "area:";
/// Where the cards with no area label are.
pub const UNSORTED: &str = "Unsorted";
// A card not done in this status is where its row says; in any other, its row says its status.
const BACKLOG: &str = "Backlog";

/// Every configured board, in the config's order.
pub fn boards(state: &State, outside: &Outside, ctx: Ctx<'_>) -> Vec<BoardView> {
    ctx.config.ordered_projects().map(|project| board(state, project, outside, ctx)).collect()
}

fn board(state: &State, project: &ProjectId, outside: &Outside, ctx: Ctx<'_>) -> BoardView {
    let live: Vec<&Card> =
        live_cards(state).into_iter().filter(|c| c.project == *project && folded_parent(state, c).is_none()).collect();
    let row = |id: &CardId| row(state, id, outside);
    let drawn = |id: &CardId| live.iter().any(|c| crate::lifecycle::carried::cards(c).any(|k| k == id));
    let mut rest: Vec<&BoardCard> = state.board.values().filter(|b| b.project == *project && !drawn(&b.id)).collect();
    rest.sort_by(|a, b| by_number(a.id.as_str(), b.id.as_str()));
    let (done, open): (Vec<&BoardCard>, Vec<&BoardCard>) = rest.into_iter().partition(|b| b.status == b.done_status);
    // A dropped card keeps its status in `archive/`: only `tasks/` holds the backlog (fac-84).
    let open: Vec<&BoardCard> = open.into_iter().filter(|b| b.file.starts_with("tasks/")).collect();
    let any = state.board.values().find(|b| b.project == *project);
    let done_name = any.map_or_else(|| "Done".to_owned(), |b| b.done_status.clone());
    BoardView {
        project: project.clone(),
        live: group("Live".to_owned(), live.iter().flat_map(|c| with_carried(state, c, row(&c.id))).collect()),
        merge: merge_queue(state, project, outside, ctx),
        areas: areas(&open, row),
        done: group(done_name, done.iter().map(|b| row(&b.id)).collect()),
    }
}

// A run's row, then a row for each card it carries, with the run's marks, its click opening the run's room (fac-563).
fn with_carried(state: &State, c: &Card, own: BoardRow) -> Vec<BoardRow> {
    let carried = c.carries.iter().map(|k| BoardRow {
        id: k.id.clone(),
        title: state.board.get(&k.id).map_or_else(|| k.title.clone(), |b| b.title.clone()),
        ran: true,
        live: own.live.clone(),
        status: None,
        room: Some(c.id.clone()),
    });
    let carried: Vec<BoardRow> = carried.collect();
    std::iter::once(own).chain(carried).collect()
}

/// A card's area: its first `area:` label's name.
pub fn area(b: &BoardCard) -> Option<&str> {
    b.labels.iter().find_map(|l| l.strip_prefix(AREA)).map(str::trim).filter(|a| !a.is_empty())
}

fn areas(cards: &[&BoardCard], row: impl Fn(&CardId) -> BoardRow) -> Vec<BoardGroup> {
    let mut by: BTreeMap<String, Vec<BoardRow>> = BTreeMap::new();
    let mut unsorted = Vec::new();
    for b in cards {
        let mut r = row(&b.id);
        r.status = Some(b.status.clone()).filter(|s| s != BACKLOG);
        match area(b) {
            Some(a) => by.entry(a.to_owned()).or_default().push(r),
            None => unsorted.push(r),
        }
    }
    let mut out: Vec<BoardGroup> = by.into_iter().map(|(name, cards)| group(name, cards)).collect();
    if !unsorted.is_empty() {
        out.push(group(UNSORTED.to_owned(), unsorted));
    }
    out
}

fn group(name: String, cards: Vec<BoardRow>) -> BoardGroup {
    BoardGroup { name, count: cards.len(), cards }
}

/// Cards currently waiting or landing in `project`'s merge queue, in queue order (fac-680).
pub(super) fn queue_cards<'a>(state: &'a State, project: &ProjectId) -> Vec<&'a Card> {
    // A card still in verification and a published PR waiting for the operator are outside the queue (fac-98).
    let in_train = |c: &&Card| {
        c.project == *project
            && c.stage == Stage::Merging
            && crate::queue::order::in_queue(c)
            && !c.alarm
            && !crate::queue::approval::waiting(c)
    };
    let mut line: Vec<&Card> = state.cards.values().filter(in_train).collect();
    line.sort_by(|a, b| (crate::queue::order::place(a), &a.id).cmp(&(crate::queue::order::place(b), &b.id)));
    line
}

fn merge_queue(state: &State, project: &ProjectId, outside: &Outside, ctx: Ctx<'_>) -> MergeQueue {
    let base = ctx.config.projects.get(project).map(|p| p.base.clone()).unwrap_or_default();
    let line = queue_cards(state, project);
    let slot = line.first().map(|c| (row(state, &c.id, outside), step_words(c, &base)));
    let waiting: Vec<(BoardRow, i64)> =
        line.iter().skip(1).map(|c| (row(state, &c.id, outside), crate::queue::order::place(c))).collect();
    MergeQueue { base, slot, in_line: waiting.len(), waiting }
}

fn step_words(c: &Card, base: &str) -> String {
    match c.landing.as_ref() {
        None | Some(Landing::Queued { .. }) => "waiting".to_owned(),
        Some(Landing::Fetch { .. }) => format!("fetching {base}"),
        Some(Landing::Prepare(_)) => "making its candidate".to_owned(),
        Some(Landing::Test(k)) if k.checked && k.queue => format!("its suite again, {base} merged in"),
        Some(Landing::Test(k)) if k.checked => "its round and its suite".to_owned(),
        Some(Landing::Test(_)) => "its candidate's checks".to_owned(),
        Some(Landing::Review(_)) => "its suite ended, its round under way".to_owned(),
        Some(Landing::Passed(_)) => "verified, waiting to land".to_owned(),
        Some(Landing::HandMerged) => "merged outside Mork".to_owned(),
        Some(Landing::Published { .. }) => "published".to_owned(),
        Some(Landing::Approval { .. }) => "waiting for its PR to merge".to_owned(),
        Some(Landing::Merge { .. }) => "merging its PR".to_owned(),
    }
}

// A review card has no board entry: its title is its run's. Working roles include a folded review's seats (fac-680).
fn row(state: &State, id: &CardId, outside: &Outside) -> BoardRow {
    let card = state.cards.get(id);
    let title = state.board.get(id).map(|b| b.title.clone()).or_else(|| card.map(|c| c.title.clone()));
    let live = card.filter(|c| c.stage.is_live()).map(|c| {
        let folded = folded_review(state, c).map(|rc| rc.id.clone());
        LiveMarks {
            light: card_light(state, c),
            stage: stage_word(state, c),
            working: state
                .agents
                .values()
                .filter(|a| {
                    let own = a.place == c.id.as_str() || a.place == crate::messages::places::spec_room(&c.id).as_str();
                    let rev = folded.as_ref().is_some_and(|rc| {
                        crate::messages::places::card_of(state, a.place.as_str()).as_ref() == Some(rc)
                    });
                    own || rev
                })
                .filter(|a| agent_light(state, &a.key, outside) == AgentLight::Working)
                .map(|a| a.role)
                .collect(),
            progress: super::pipeline::progress(state, c, outside),
        }
    });
    BoardRow {
        id: id.clone(),
        title: title.unwrap_or_default(),
        ran: card.is_some() || state.unloaded.cards.contains(id),
        live,
        status: None,
        room: None,
    }
}
