//! The decisions, as pure functions: `decide(&State, Event, now) -> Decision` (1.12.1).
//! No clock, no randomness, no file, process, network or database.
#![forbid(unsafe_code)]
#![deny(clippy::wildcard_enum_match_arm)]

pub mod agents;
pub mod board;
pub mod decide;
pub mod door;
pub mod internals;
pub mod lifecycle;
pub mod maintenance;
pub mod memory;
pub mod messages;
pub mod post;
pub mod queue;
pub mod reviews;
mod search_text;
pub mod state;
pub mod views;
