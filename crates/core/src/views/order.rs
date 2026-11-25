use std::cmp::Ordering;

use factory_types::card::Card;

use crate::state::State;

/// The live cards, by start, then by id as numbers.
pub(super) fn live_cards(state: &State) -> Vec<&Card> {
    let mut cards: Vec<&Card> = state.cards.values().filter(|c| c.stage.is_live()).collect();
    cards.sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then_with(|| by_number(a.id.as_str(), b.id.as_str())));
    cards
}

/// Ids compared as numbers (`ex-12` before `ex-12.1` before `ex-100`): runs of digits by
/// value, the rest as text.
pub fn by_number(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| {
        let mut out: Vec<(bool, String)> = Vec::new();
        for ch in s.chars() {
            let digit = ch.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(ch),
                _ => out.push((digit, ch.to_string())),
            }
        }
        out
    };
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(&pb) {
        let o = match (x, y) {
            ((true, n), (true, m)) => n.parse::<u64>().unwrap_or(0).cmp(&m.parse::<u64>().unwrap_or(0)),
            ((_, n), (_, m)) => n.cmp(m),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len())
}
