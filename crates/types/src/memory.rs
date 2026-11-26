//! The agents' memories (fac-159): a general one and one per project, each a set of short files
//! the engine alone writes, every seat's read and every write counted.

use serde::{Deserialize, Serialize};

/// The memory of no project: how Mork and the operator work.
pub const GENERAL: &str = "general";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFile {
    /// `general`, or a project's name.
    pub memory: String,
    /// Unique within its memory.
    pub name: String,
    pub description: String,
    pub text: String,
    /// Who wrote it last, as a sender's label.
    pub writer: String,
    /// Who created it, as a sender's label; a rewrite keeps it (fac-598).
    #[serde(default)]
    pub origin: String,
    pub written_ms: i64,
    pub reads: u64,
    #[serde(default)]
    pub last_read_ms: Option<i64>,
}

impl MemoryFile {
    /// Its key in the state: `<memory>/<name>`.
    pub fn key(&self) -> String {
        key(&self.memory, &self.name)
    }
}

/// A file's key in the state, from its memory and name.
pub fn key(memory: &str, name: &str) -> String {
    format!("{memory}/{name}")
}

/// A memory's cumulative counts, which outlive its files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub memory: String,
    pub written: u64,
    pub read: u64,
    pub deleted: u64,
    /// When the operator last sent the memory to review (fac-610); none until a send.
    #[serde(default)]
    pub reviewed_ms: Option<i64>,
}

/// A memory as main's Memory tab shows it, the data of `memory.stats`: its counts, then its files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryView {
    pub tally: Tally,
    /// The last review as a local date, `DD.MM.YYYY`; none until a send (fac-610).
    #[serde(default)]
    pub reviewed: Option<String>,
    /// By name.
    pub files: Vec<FileRow>,
}

/// A file's row in the Memory tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRow {
    pub name: String,
    pub description: String,
    pub reads: u64,
    #[serde(default)]
    pub last_read_ms: Option<i64>,
}
