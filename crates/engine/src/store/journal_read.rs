use std::collections::BTreeSet;

use factory_core::views::journal::{self, Anchor, Page, Read, Row};
use factory_types::ProjectId;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::{Result, Store};

struct Reader<'a> {
    conn: &'a Connection,
    read: &'a Read,
    jobs: BTreeSet<String>,
}

impl Store {
    /// Reads a journal page and its permission, job and message references in one read transaction.
    pub fn history_journal(
        &self,
        scope: Option<&ProjectId>,
        read: &Read,
    ) -> Result<std::result::Result<Value, String>> {
        let tx = self.conn.unchecked_transaction()?;
        let sql = "SELECT json_extract(data, '$.project') FROM entities WHERE kind = 'card' AND id = ?1";
        let target: Option<String> = tx.query_row(sql, params![read.card], |r| r.get(0)).optional()?;
        let Some(target) = target else { return Ok(Err(format!("no card `{}`", read.card))) };
        if !journal::reaches(scope, &ProjectId::new(target)) {
            return Ok(Err(format!("card `{}` is outside your project's journal reach", read.card)));
        }
        let reader = Reader { conn: &tx, read, jobs: card_jobs(&tx, &read.card)? };
        let pivot = match read.anchor {
            Anchor::Seq(n) | Anchor::Before(n) => n,
            Anchor::At(time) => reader.at(time)?,
        };
        let mut page = Page::new(read, pivot);
        reader.side(&mut page, pivot, 0)?;
        reader.side(&mut page, pivot, 1)?;
        let answer = page.answer();
        tx.commit()?;
        Ok(answer)
    }
}

fn card_jobs(conn: &Connection, card: &str) -> Result<BTreeSet<String>> {
    let sql = "SELECT id FROM jobs WHERE json_extract(data, '$.purpose.card') = ?1 \
               OR substr(json_extract(data, '$.owner'), 1, length(?1) + 1) = ?1 || '/'";
    let mut stmt = conn.prepare(sql)?;
    Ok(stmt.query_map(params![card], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

impl Reader<'_> {
    fn belongs(&self, card: Option<&str>, data: &Value) -> bool {
        card == Some(self.read.card.as_str()) || card.is_none() && journal::cardless(&self.read.card, &self.jobs, data)
    }

    fn at(&self, time: i64) -> Result<i64> {
        let sql = "SELECT seq, card, kind, data FROM events \
                   WHERE (card = ?1 OR card IS NULL) AND time >= ?2 ORDER BY seq";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params![self.read.card, time])?;
        while let Some(r) = rows.next()? {
            if !self.read.includes(&r.get::<_, String>(2)?) {
                continue;
            }
            let data: Value = serde_json::from_str(&r.get::<_, String>(3)?)?;
            if self.belongs(r.get::<_, Option<String>>(1)?.as_deref(), &data) {
                return Ok(r.get(0)?);
            }
        }
        Ok(i64::MAX)
    }

    fn side(&self, page: &mut Page, pivot: i64, side: usize) -> Result<()> {
        let (cmp, order) = if side == 0 { ("<", "DESC") } else { (">=", "ASC") };
        let sql = format!(
            "SELECT seq, time, card, kind, data FROM events \
             WHERE (card = ?1 OR card IS NULL) AND seq {cmp} ?2 ORDER BY seq {order}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![self.read.card, pivot])?;
        while let Some(r) = rows.next()? {
            let kind = r.get::<_, String>(3)?;
            if !self.read.includes(&kind) {
                continue;
            }
            let data: Value = serde_json::from_str(&r.get::<_, String>(4)?)?;
            if !self.belongs(r.get::<_, Option<String>>(2)?.as_deref(), &data) || !page.wants(side) {
                continue;
            }
            let message = self.message(&kind, &data)?;
            page.offer(side, Row { seq: r.get(0)?, time: r.get(1)?, kind, data, message });
        }
        Ok(())
    }

    fn message(&self, kind: &str, data: &Value) -> Result<Option<Value>> {
        let field = match kind {
            "message" => "seq",
            "reply" => "hand",
            _ => return Ok(None),
        };
        let Some(seq) = data.get(field).and_then(Value::as_i64) else { return Ok(None) };
        let sql = "SELECT data FROM messages WHERE seq = ?1";
        let stored: Option<String> = self.conn.query_row(sql, params![seq], |r| r.get(0)).optional()?;
        stored.map(|data| Ok(journal::message(serde_json::from_str(&data)?))).transpose()
    }
}
