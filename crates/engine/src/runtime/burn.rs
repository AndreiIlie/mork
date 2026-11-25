// Rolling 7-day burn reads for `usage`: queries the journal only when `usage` is requested.

use std::collections::{BTreeMap, BTreeSet};

use factory_core::state::State;
use factory_core::views::burn::{self, Row, WEEK_MS};
use factory_types::CardId;
use factory_types::usage::{Account, RecentBurn};

use crate::store::{JournalRow, Store};

fn as_rows(rows: &[JournalRow]) -> Vec<Row<'_>> {
    rows.iter().map(|(t, c, k, d)| (*t, c.as_deref(), k.as_str(), d)).collect()
}

pub(super) fn recent(
    state: &State,
    store: &Store,
    (now, tz_offset_min): (i64, i32),
    active: Option<&Account>,
    saved: &BTreeMap<String, Account>,
) -> RecentBurn {
    let since = now.saturating_sub(WEEK_MS);
    // The daily series reaches back further than the week: the week's figures read their part of its rows (fac-696).
    let start = burn::series_start(now, tz_offset_min).min(since);
    let costs = store.events_since("cost", start).unwrap_or_default();
    let landed = store.events_since("card.landed", start).unwrap_or_default();
    let stages = store.events_since("card.stage", since).unwrap_or_default();
    let daily = burn::daily(now, tz_offset_min, &as_rows(&costs), &as_rows(&landed));
    let costs: Vec<JournalRow> = costs.into_iter().filter(|(t, ..)| *t >= since).collect();
    let cost_refs = as_rows(&costs);
    let stage_refs = as_rows(&stages);
    let ended = burn::ended_runs(&stage_refs);

    let mut all_cards: BTreeSet<String> = costs.iter().filter_map(|(_, c, _, _)| c.clone()).collect();
    let ended_cards: BTreeSet<String> = ended.iter().map(|(c, _, _)| c.clone()).collect();
    all_cards.extend(ended_cards.iter().cloned());

    let parents = resolve_parents(state, store, &all_cards);
    let owned_rows: BTreeMap<String, Vec<JournalRow>> = ended_cards
        .into_iter()
        .map(|card| {
            let rows = store.card_burn_events(&card).unwrap_or_default();
            (card, rows)
        })
        .collect();
    let card_rows: BTreeMap<String, Vec<Row<'_>>> =
        owned_rows.iter().map(|(card, rows)| (card.clone(), as_rows(rows))).collect();
    let pools = super::usage::week_pools(state, now, active, saved);
    RecentBurn { daily, ..burn::recent(since, &cost_refs, &stage_refs, &card_rows, &parents, &pools) }
}

fn resolve_parents(state: &State, store: &Store, cards: &BTreeSet<String>) -> BTreeMap<String, String> {
    let mut parents = BTreeMap::new();
    let mut missing = Vec::new();
    for id in cards {
        match state.cards.get(&CardId::new(id.as_str())) {
            Some(c) => {
                if let Some(of) = factory_core::reviews::gate::subject_of(c) {
                    parents.insert(id.clone(), of.as_str().to_owned());
                }
            }
            None => missing.push(id.clone()),
        }
    }
    if let Ok(stored) = store.review_parents(&missing) {
        parents.extend(stored);
    }
    parents
}
