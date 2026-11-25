//! Identities, as newtypes (1.12.5).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

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

newtype!(
    /// A job's id: `<yyyymmdd-hhmmss>-<owner>-<4 hex>` (1.4.2).
    JobId
);
newtype!(
    /// A card's id, as its board names it in lowercase (`ex-550`).
    CardId
);
newtype!(
    /// An agent: a seat in a role at a place (`ex-550/foreman`, `main/echo`).
    AgentKey
);
newtype!(
    /// A git commit's full hash.
    Sha
);
newtype!(
    /// A place: `main`, a card's room (`ex-550`), a DM (`ex-550/foreman/dm`), a test place.
    PlaceId
);
newtype!(
    /// A seat of the config (`foreman`, `reviewer`, `echo`).
    SeatName
);
newtype!(
    /// A CLI's session: Claude's session id, or Codex's thread id.
    SessionId
);

newtype!(
    /// An ask: `<place>/<kind>/<n>` (`ex-550/approve-brief/1`).
    AskId
);
newtype!(
    /// A project of the config (`example`).
    ProjectId
);
newtype!(
    /// An outward action's id (2.5).
    ActionId
);
newtype!(
    /// A test or engine lease (4.9): `test-<n>`, `engine-<card>`.
    LeaseId
);

impl Sha {
    /// The first seven characters, as the screen shows a commit.
    pub fn short(&self) -> &str {
        self.0.get(..7).unwrap_or(&self.0)
    }
}
