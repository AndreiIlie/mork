//! A failed candidate's news (fac-164): its log's copies, sent unmarked (fac-311).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::card::FixImport;
use crate::ids::CardId;
use crate::reply::Role;

/// A failed candidate's news, until it is told: its log's copies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteNews {
    /// Its whole log copied into each told card's clone: the copy's path, by card.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub logs: BTreeMap<CardId, String>,
}

/// One delivery of a failure's news.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "kebab-case")]
pub enum Told {
    /// Said to Andrei in a card's room.
    Andrei { card: CardId, text: String },
    /// A request to a card's seat, waking it.
    Agent { card: CardId, role: Role, headline: String, text: String },
    /// A request to a card's foreman held until the base's tip reaches its clone (fac-144).
    Fix { card: CardId, fix: FixImport },
}
