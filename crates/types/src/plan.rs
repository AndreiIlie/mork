//! A card's plan (fac-36, fac-128): its deliverables, each with the tasks its foreman posted for it.

use serde::{Deserialize, Serialize};

/// A card's plan (fac-36, fac-128): its deliverables in order, the one being worked on now, and
/// each one's tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedPlan")]
pub struct Plan {
    pub deliverables: Vec<Deliverable>,
    /// The current deliverable, as its index in `deliverables`.
    pub current: usize,
}

/// One deliverable of a card, named by its foreman (fac-128), its tasks the plan its foreman posted
/// for it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deliverable {
    pub name: String,
    pub tasks: Vec<String>,
    /// The current task, as its index in `tasks`.
    pub current: usize,
}

/// The one deliverable of a plan posted before any were cut, or saved before there were any.
pub const WHOLE_CARD: &str = "The whole card";

impl Plan {
    /// A plan of one deliverable, the whole card, holding these tasks.
    pub fn whole_card(tasks: Vec<String>, current: usize) -> Self {
        Self { deliverables: vec![Deliverable { name: WHOLE_CARD.to_owned(), tasks, current }], current: 0 }
    }

    /// The deliverable being worked on now.
    pub fn deliverable(&self) -> Option<&Deliverable> {
        self.deliverables.get(self.current)
    }
}

// A card an older build saved holds one flat plan (fac-36): it loads as the whole card's.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedPlan {
    Deliverables { deliverables: Vec<Deliverable>, current: usize },
    Flat { tasks: Vec<String>, current: usize },
}

impl From<SavedPlan> for Plan {
    fn from(saved: SavedPlan) -> Self {
        match saved {
            SavedPlan::Deliverables { deliverables, current } => Self { deliverables, current },
            SavedPlan::Flat { tasks, current } => Self::whole_card(tasks, current),
        }
    }
}
