//! What a card's state says of how its run goes: who makes it, whether it is agreeing its specification or may write its
//! clone, its mode's word, and whether its build started with nothing to clarify.

use mork_types::card::{ApprovedVia, Card, Mode, Stage};
use mork_types::reply::Role;

/// Whether the card is agreeing its specification: a full card started, or at its brief.
pub fn agreeing(card: &Card) -> bool {
    matches!(card.stage, Stage::Starting | Stage::Brief) && card.mode == Mode::Full
}

/// The room's seat for the card's current step: the foreman throughout a no-planner card (fac-616).
pub fn holder(card: &Card) -> Role {
    if agreeing(card) && !card.no_planner { Role::Planner } else { Role::Foreman }
}

/// The seat that settles the specification, including after approval.
pub fn specifier(card: &Card) -> Role {
    if card.mode == Mode::Full && !card.no_planner { Role::Planner } else { Role::Foreman }
}

/// Whether a card's agents may write its clone: not a report card's (section 4).
pub fn clone_writable(card: &Card) -> bool {
    card.mode != Mode::Report
}

pub fn mode_word(m: Mode) -> &'static str {
    match m {
        Mode::Full => "full",
        Mode::Light => "light",
        Mode::Report => "report",
        Mode::Review => "review",
    }
}

/// Whether the card's build started with nothing to clarify, on no approval of the operator's (fac-24).
pub fn clear(c: &Card) -> bool {
    c.approval.as_ref().is_some_and(|a| a.via == ApprovedVia::Clear)
}
