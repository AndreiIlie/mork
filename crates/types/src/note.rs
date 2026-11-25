//! A project's notes to its orchestrator (fac-287): what a card leaves behind that no other seat
//! reads, survey answers, code smells, feedback and handoffs, in one place. Memory stays apart.

use serde::{Deserialize, Serialize};

/// One note. It moves no card and requires no fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    /// None: nothing accounts for it, so it is the factory's project's.
    pub project: Option<crate::ProjectId>,
    pub card: Option<crate::CardId>,
    pub seat: Option<crate::AgentKey>,
    pub time: i64,
    #[serde(flatten)]
    pub what: NoteWhat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NoteWhat {
    /// An answer of the sampled survey, kept from before the closing turn replaced it: nothing
    /// new is of this kind.
    Survey { from: String, text: String },
    /// A code smell in code its seat's work doesn't change.
    Smell { file: String, line: u64, what: String, why: String },
    /// A bug, an issue or a suggestion.
    Feedback { text: String },
    /// What a seat learned that would save a later agent time (fac-592): its orchestrator triages it into memory
    /// or a skill.
    Lesson { text: String },
    /// A grouped cleanup ready to become a card.
    Proposal { title: String, text: String, files: Vec<String> },
    /// The files a card left under `.git/factory/handoff/`, preserved in `dir`.
    Handoff { run: u32, dir: String, files: Vec<HandoffFile>, skipped: Vec<String> },
}

/// A preserved file, its path relative to its handoff's `dir`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffFile {
    pub path: String,
    pub bytes: u64,
}

impl NoteWhat {
    /// The kind, as `notes` and the window name it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Survey { .. } => "survey",
            Self::Smell { .. } => "smell",
            Self::Feedback { .. } => "feedback",
            Self::Lesson { .. } => "lesson",
            Self::Proposal { .. } => "proposal",
            Self::Handoff { .. } => "handoff",
        }
    }
}

/// The kinds `notes.add` files; a handoff is the engine's to record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NewNote {
    Feedback,
    Lesson,
    Smell,
    Proposal,
}

/// A project's notes as main's Notes tab shows them, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectNotes {
    pub project: crate::ProjectId,
    pub notes: Vec<Note>,
}
