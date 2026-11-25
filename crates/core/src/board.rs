//! The boards (4.18): Backlog.md's cards and configs (F-K1–F-K6), one parser and writer that keeps what a card says;
//! the board commands, each write's content and the card's ticket notes are decided here too.

mod commands;
mod edit;
mod file;
pub(crate) mod notes;
pub(crate) mod retry;
pub(crate) mod tickets;
pub mod writes;

pub use commands::command;
pub use edit::{Fields, add_note, check_fields, close, new_card, no_markers, one_line, set_fields};
pub use file::{CardFile, config, next_id, number, parse, read, valid_id};

/// What is wrong with a board's file, or with what would be written to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    /// The file isn't in the board's shape (F-K1–F-K4): how.
    Malformed(&'static str),
    /// What would be written can't be: why.
    Refused(&'static str),
    /// A field's value can't be written: the field, and why.
    Field(&'static str, &'static str),
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(why) | Self::Refused(why) => write!(f, "{why}"),
            Self::Field(field, why) => write!(f, "`{field}` {why}"),
        }
    }
}

impl std::error::Error for BoardError {}

impl From<BoardError> for String {
    fn from(e: BoardError) -> Self {
        e.to_string()
    }
}

const BEGIN: &str = "<!-- SECTION:DESCRIPTION:BEGIN -->";
const END: &str = "<!-- SECTION:DESCRIPTION:END -->";
