//! The boards (4.18): the engine reads and writes their files; their format, one parser and
//! writer with no I/O, is `core::board`, so `core` decides what a board write holds (M3.16).

mod recovery;
mod writes;

use mork_core::board;
use mork_types::action::{Action, BoardEdit};
use mork_types::card::BoardCard;
use mork_types::config::BoardConfig;
use mork_types::work::{ActionFailure, Found};
use mork_types::{CardId, ProjectId};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) trait BoardRead {
    fn snapshot(&self, name: &ProjectId) -> (Vec<BoardCard>, Option<String>);
    fn prepare(&self, file: &str, edit: &BoardEdit, now: i64) -> Option<Value>;
    fn found(&self, action: &Action) -> Found;
}

pub(crate) trait BoardWrite {
    fn write(&self, action: &Action, now: i64) -> Result<Value, ActionFailure>;
}

struct Markdown {
    backlog: PathBuf,
}

fn provider(config: &BoardConfig) -> Markdown {
    match config {
        BoardConfig::Markdown { path } => Markdown { backlog: Path::new(path).join("backlog") },
    }
}

pub(crate) fn read(config: &BoardConfig) -> impl BoardRead {
    provider(config)
}
pub(crate) fn write(config: &BoardConfig) -> impl BoardWrite {
    provider(config)
}

impl BoardRead for Markdown {
    fn snapshot(&self, name: &ProjectId) -> (Vec<BoardCard>, Option<String>) {
        let parsed = std::fs::read_to_string(self.backlog.join("config.yml")).ok().and_then(|t| board::config(&t).ok());
        let board_prefix = parsed.as_ref().map(|(p, _)| p.clone());
        let mut out = Vec::new();
        let (prefix, done) = parsed.unwrap_or_default();
        for dir in ["tasks", "completed", "archive/tasks"] {
            for e in std::fs::read_dir(self.backlog.join(dir)).into_iter().flatten().flatten() {
                let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
                let Ok(f) = board::parse(&text) else { continue };
                // Its whole text and its board's prefix: what `core` makes a write from (M3.16).
                out.push(BoardCard {
                    id: CardId::new(f.id.to_lowercase()),
                    project: name.clone(),
                    title: f.title,
                    description: f.description,
                    acceptance: f.acceptance,
                    status: f.status,
                    labels: f.labels,
                    done_status: done.clone(),
                    file: format!("{dir}/{}", e.file_name().to_string_lossy()),
                    text,
                    prefix: prefix.clone(),
                });
            }
        }
        (out, board_prefix)
    }

    fn prepare(&self, file: &str, edit: &BoardEdit, now: i64) -> Option<Value> {
        let before = std::fs::read_to_string(self.backlog.join(file)).ok()?;
        let (after, _) = writes::edited(now, &self.backlog, file, &before, edit).ok()?;
        Some(
            json!({"before": crate::files::sha256_hex(before.as_bytes()), "after": crate::files::sha256_hex(after.as_bytes())}),
        )
    }

    fn found(&self, action: &Action) -> Found {
        recovery::found(&self.backlog, action)
    }
}
