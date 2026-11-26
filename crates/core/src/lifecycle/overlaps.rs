//! Overlaps (4.15): cards of one project and base sharing files, marked with each other.

use std::collections::BTreeSet;

use mork_types::CardId;
use mork_types::reply::Role;

use crate::decide::Ctx;
use crate::messages::requests::tell_agent;
use crate::post::Poster;

fn named(files: &[String]) -> String {
    let mut out = files.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    if files.len() > 8 {
        out.push_str(&format!(" and {} more", files.len() - 8));
    }
    out
}

/// Compares a card's changed files with every other live card's of its project.
pub(super) fn compare(p: &mut Poster<'_>, id: &CardId, _ctx: Ctx<'_>) {
    let Some(mut card) = p.card(id) else { return };
    let others: Vec<CardId> = p
        .state
        .cards
        .values()
        .filter(|c| &c.id != id && c.project == card.project && c.stage.is_live())
        .map(|c| c.id.clone())
        .collect();
    let mine: BTreeSet<&String> = card.changed.iter().collect();
    for other in others {
        let Some(mut o) = p.card(&other) else { continue };
        let shared: Vec<String> = o.changed.iter().filter(|f| mine.contains(f)).cloned().collect();
        let before = card.overlaps.get(&other).cloned();
        if shared.is_empty() {
            card.overlaps.remove(&other);
            o.overlaps.remove(id);
            p.put_card(o);
            continue;
        }
        card.overlaps.insert(other.clone(), shared.clone());
        o.overlaps.insert(id.clone(), shared.clone());
        p.put_card(o);
        if before.as_ref() != Some(&shared) {
            let text = |with: &CardId| format!("{with} changes the same files: {}", named(&shared));
            tell_agent(p, id, Role::Foreman, "Overlap", &text(&other));
            tell_agent(p, &other, Role::Foreman, "Overlap", &text(id));
        }
    }
    p.put_card(card);
}

/// A card ended: its marks go with it.
pub fn clear(p: &mut Poster<'_>, id: &CardId) {
    let Some(mut card) = p.card(id) else { return };
    let marked: Vec<CardId> = card.overlaps.keys().cloned().collect();
    card.overlaps.clear();
    p.put_card(card);
    for other in marked {
        if let Some(mut o) = p.card(&other) {
            o.overlaps.remove(id);
            p.put_card(o);
        }
    }
}
