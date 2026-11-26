// What a start asks of history, kept in `meta` rows as it is written (fac-336), so the start reads
// a row each instead of every event or door call: once-only lines said, pins tried, door news owed.

use std::collections::BTreeSet;

use mork_core::decide::Journal;
use mork_core::state::Change;
use mork_types::action::{Action, ActionWhat};
use rusqlite::{Transaction, params};

use super::{Result, Store};

const SAID: &str = "said:";
const PIN: &str = "pin.tried:";
const OWED: &str = "door.owed:";

/// Keeps a written door call's row: there while its requester is owed its news.
pub(super) fn change(tx: &Transaction<'_>, c: &Change) -> Result<()> {
    if let Change::PutAction(a) = c
        && matches!(a.what, ActionWhat::Door(_))
    {
        owed(tx, a)?;
    }
    Ok(())
}

fn owed(tx: &Transaction<'_>, a: &Action) -> Result<()> {
    let key = format!("{OWED}{}", a.id);
    if mork_core::door::owed_news(a) {
        tx.execute("INSERT OR IGNORE INTO meta (key, value) VALUES (?1, '1')", params![key])?;
    } else {
        tx.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
    }
    Ok(())
}

/// Keeps a journaled once-only line's key, and a pin tried's commit.
pub(super) fn journal(tx: &Transaction<'_>, j: &Journal) -> Result<()> {
    let (prefix, field) = match j.kind.as_str() {
        "engine.says" => (SAID, "key"),
        "update.pinned" | "update.refused" => (PIN, "commit"),
        _ => return Ok(()),
    };
    if let Some(v) = j.data.get(field).and_then(serde_json::Value::as_str) {
        tx.execute("INSERT OR IGNORE INTO meta (key, value) VALUES (?1, '1')", params![format!("{prefix}{v}")])?;
    }
    Ok(())
}

impl Store {
    /// Whether the engine has said the line with this once-only key (`engine.says`'s `key`).
    pub fn said(&self, key: &str) -> Result<bool> {
        Ok(self.meta(&format!("{SAID}{key}"))?.is_some())
    }

    /// Whether an update's pin was tried for this commit: pinned, or refused (fac-19).
    pub fn pin_tried(&self, commit: &str) -> Result<bool> {
        Ok(self.meta(&format!("{PIN}{commit}"))?.is_some())
    }

    /// The door calls whose news is owed, by id.
    pub(super) fn owed_doors(&self) -> Result<BTreeSet<String>> {
        let mut stmt = self.conn.prepare("SELECT key FROM meta WHERE key GLOB ?1")?;
        let keys = stmt.query_map(params![format!("{OWED}*")], |r| r.get::<_, String>(0))?;
        let keys = keys.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys.iter().filter_map(|k| k.strip_prefix(OWED)).map(str::to_owned).collect())
    }
}
