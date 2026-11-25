use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use super::{JournalRow, Result, Store};

impl Store {
    fn journal_rows(&self, filter: &str, values: impl rusqlite::Params) -> Result<Vec<JournalRow>> {
        let mut stmt =
            self.conn.prepare(&format!("SELECT time, card, kind, data FROM events {filter} ORDER BY seq"))?;
        let rows = stmt.query_map(values, |r| {
            let data: String = r.get(3)?;
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, serde_json::from_str(&data).unwrap_or(serde_json::Value::Null)))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The journal's events, oldest first.
    pub fn events(&self) -> Result<Vec<JournalRow>> {
        self.journal_rows("", [])
    }

    /// One card's journal events, oldest first (its record, 2.9.1).
    pub fn card_events(&self, card: &str) -> Result<Vec<JournalRow>> {
        self.journal_rows("WHERE card = ?1", [card])
    }

    /// One card's start, stage, cost and review-link events, oldest first, avoiding message deserialization.
    pub fn card_burn_events(&self, card: &str) -> Result<Vec<JournalRow>> {
        self.journal_rows(
            "WHERE card = ?1 AND kind IN ('card.start', 'card.stage', 'cost', 'review.adopted', 'review.ended')",
            [card],
        )
    }

    /// The parent card id (`review.of`) stored on each unloaded review card in `cards`.
    pub fn review_parents(&self, cards: &[String]) -> Result<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT json_extract(data, '$.review.of') FROM entities WHERE kind = 'card' AND id = ?1")?;
        for id in cards {
            let parent: Option<Option<String>> = stmt.query_row([id.as_str()], |r| r.get(0)).optional()?;
            if let Some(Some(of)) = parent.filter(|p| p.as_deref().is_some_and(|s| !s.is_empty())) {
                out.insert(id.clone(), of);
            }
        }
        Ok(out)
    }

    /// The journal's events of a kind, oldest first.
    pub fn events_of(&self, kind: &str) -> Result<Vec<JournalRow>> {
        self.journal_rows("WHERE kind = ?1", [kind])
    }

    /// The journal's events of a kind from `time` on, oldest first.
    pub fn events_since(&self, kind: &str, time: i64) -> Result<Vec<JournalRow>> {
        self.journal_rows("WHERE kind = ?1 AND time >= ?2", rusqlite::params![kind, time])
    }
}
