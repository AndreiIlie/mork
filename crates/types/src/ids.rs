//! Identities, as newtypes (1.12.5).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! impl_id {
    ($name:ident) => {
        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        // Read as text wherever text is all that's needed; it is made only by `new`.
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        // Looked up by its text: ordered and hashed as that text is.
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
    };
}

/// A job's id: `<yyyymmdd-hhmmss>-<owner>-<4 hex>` (1.4.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub String);
impl_id!(JobId);
/// A card's id, as its board names it in lowercase (`ex-550`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardId(pub String);
impl_id!(CardId);
/// An agent: a seat in a role at a place (`ex-550/foreman`, `main/echo`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentKey(pub String);
impl_id!(AgentKey);
/// A git commit's full hash.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha(pub String);
impl_id!(Sha);
/// A place: `main`, a card's room (`ex-550`), a DM (`ex-550/foreman/dm`), a test place.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaceId(pub String);
impl_id!(PlaceId);
/// A seat of the config (`foreman`, `reviewer`, `echo`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SeatName(pub String);
impl_id!(SeatName);
/// A CLI's session: Claude's session id, or Codex's thread id.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);
impl_id!(SessionId);

/// An ask: `<place>/<kind>/<n>` (`ex-550/approve-brief/1`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AskId(pub String);
impl_id!(AskId);
/// A project of the config (`example`).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);
impl_id!(ProjectId);
/// An outward action's id (2.5).
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(pub String);
impl_id!(ActionId);
/// A test or engine lease (4.9): `test-<n>`, `engine-<card>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeaseId(pub String);
impl_id!(LeaseId);

impl Sha {
    /// The first seven characters, as the screen shows a commit.
    pub fn short(&self) -> &str {
        self.0.get(..7).unwrap_or(&self.0)
    }
}
