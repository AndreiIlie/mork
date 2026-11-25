//! The window's own state (5.1.5) comes back across a reload or restart from `ui-session.json`,
//! which the app reads and writes; the window only hands it over.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// `ui-session.json`'s format (2.3).
pub const FORMAT: u32 = 1;

/// Main's tabs (5.8), the Memory tab's fac-159's; Engine was Chat, and Notes was Surveys and Code
/// smells (fac-287).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Board,
    /// Main's conversation; an older file's `chat` opens it.
    #[serde(alias = "chat")]
    Engine,
    Memory,
    /// An older file's Surveys or Code smells tab opens it.
    #[serde(alias = "surveys", alias = "smells")]
    Notes,
    Projects,
}

impl Tab {
    /// In the order drawn.
    pub const ALL: [Self; 5] = [Self::Board, Self::Engine, Self::Memory, Self::Notes, Self::Projects];

    /// The tab `step` places on, round the ends.
    pub fn beside(self, step: i64) -> Self {
        let at = Self::ALL.iter().position(|t| *t == self).unwrap_or_default() as i64;
        Self::ALL[(at + step).rem_euclid(Self::ALL.len() as i64) as usize]
    }
}

/// A field this build doesn't find takes its default: an older file still opens where it was
/// (M4.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSession {
    pub format: u32,
    /// The place shown.
    pub place: String,
    pub tab: Tab,
    pub drafts: BTreeMap<String, String>,
    /// The images pasted into each place's draft, as the paths of the window's copies (fac-31).
    pub draft_images: BTreeMap<String, Vec<String>>,
    /// The messages unfolded, by place.
    pub unfolded: BTreeMap<String, BTreeSet<u64>>,
    /// The places whose messages are all unfolded (Tab).
    pub all_unfolded: BTreeSet<String>,
    pub scroll: BTreeMap<String, f32>,
    /// The cards folded into their heading in the rooms column (fac-96, fac-168).
    pub collapsed: BTreeSet<String>,
    /// The side column's sections open (true) or folded, by name: one not named takes its
    /// default, the same on every card (fac-105).
    pub sections: BTreeMap<String, bool>,
    /// Where the window last was on the desktop: it opens maximized on that monitor (5.1.1,
    /// M4.16).
    pub position: Option<(i32, i32)>,
}

impl Default for UiSession {
    fn default() -> Self {
        Self {
            format: FORMAT,
            place: "main".into(),
            tab: Tab::Board,
            drafts: BTreeMap::new(),
            draft_images: BTreeMap::new(),
            unfolded: BTreeMap::new(),
            all_unfolded: BTreeSet::new(),
            scroll: BTreeMap::new(),
            collapsed: BTreeSet::new(),
            sections: BTreeMap::new(),
            position: None,
        }
    }
}
