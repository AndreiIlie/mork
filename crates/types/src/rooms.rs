//! The rooms column's rows (5.6.1, fac-168) and the kinds of place, re-exported by `view`.

use serde::{Deserialize, Serialize};

use crate::ask::Color;
use crate::card::Mode;
use crate::reply::Role;
use crate::view::AgentLight;
use crate::{AgentKey, CardId, PlaceId};

/// One row of the rooms column (5.6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomRow {
    pub place: PlaceId,
    pub kind: PlaceKind,
    /// The card's light; none for main, a DM or a card with no light.
    pub light: Option<Color>,
    /// The stage's word (`build`, `pr`), empty for main.
    pub stage: String,
    pub paused: bool,
    /// One per agent of the card, in its state's colour.
    pub squares: Vec<AgentLight>,
    /// A reviewer's topic (fac-559), shown beside its name; none for any other row and for reviewers saved before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// The project group and card heading it sits under (fac-168); none for main and the
    /// survey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under: Option<Under>,
}

/// Where a card's place sits in the rooms column (fac-168): its project's group, then its card's
/// heading, on the card's own side (its room, its DMs and children) or its review twin's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Under {
    /// The group: its project's name, or `<project> code reviews` for a colleague's review card.
    pub project: String,
    /// The heading's card: ours, or a colleague's PR or branch review card with no card of ours.
    pub card: CardId,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub review: bool,
    /// A ticketed project heading card's PR and DevOps tasks, as `PR 123 DevOps 1234 1235` (fac-362);
    /// empty when it has neither, and for any other project.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// The row's own card's mode, a review twin's `review`: what its room is called (fac-207).
    #[serde(default, skip_serializing_if = "is_full")]
    pub mode: Mode,
}

fn is_full(m: &Mode) -> bool {
    *m == Mode::Full
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum PlaceKind {
    Main,
    /// A card's build room: its own id.
    Card {
        card: CardId,
    },
    /// A full card's spec room, the operator's and its planner's (fac-633).
    Spec {
        card: CardId,
    },
    /// A full card's challenge place: its foreman's and planner's exchange, which the window shows nowhere.
    Challenge {
        card: CardId,
    },
    Dm {
        card: CardId,
        role: Role,
    },
    /// A foreman's child (fac-94): its DM, nested under its foreman.
    Child {
        card: CardId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<Role>,
        agent: AgentKey,
    },
    /// A test place, or a retired standalone research's.
    Other,
}
