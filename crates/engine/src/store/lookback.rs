use mork_core::maintenance::changelog;
use mork_types::ProjectId;
use mork_types::action::Action;
use rusqlite::params;
use serde::Serialize;
use serde_json::{Value, json};

use super::{Result, Store};

/// A landing's known bounds and the submitted head that identifies its card's work.
#[derive(Serialize)]
pub(crate) struct Landing {
    pub card: String,
    pub time_ms: i64,
    pub head: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
}

impl Store {
    /// A project's recorded landings, including those whose old records only know the head.
    pub(crate) fn lookback_landings(&self, project: &ProjectId) -> Result<Vec<Landing>> {
        let sql = "SELECT landed.card, landed.time, landed.data, (
                       SELECT json_group_array(json(kept.data)) FROM events AS done
                       JOIN actions AS kept ON kept.id = json_extract(done.data, '$.id')
                       WHERE done.kind = 'action.done' AND done.card = landed.card AND done.time = landed.time
                         AND (kept.natural_key LIKE 'move-base:%' OR kept.natural_key LIKE 'merge:%')
                   ) FROM events AS landed JOIN entities AS card ON card.kind = 'card' AND card.id = landed.card
                   WHERE landed.kind = 'card.landed' AND json_extract(card.data, '$.project') = ?1
                   ORDER BY landed.seq";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params![project.as_str()])?;
        let mut landings = Vec::new();
        while let Some(row) = rows.next()? {
            let card: String = row.get(0)?;
            let time_ms = row.get(1)?;
            let data: Value = serde_json::from_str(&row.get::<_, String>(2)?)?;
            let actions: Vec<Action> = serde_json::from_str(&row.get::<_, String>(3)?)?;
            let (before, after) = changelog::bounds(&data, &actions, &card, time_ms);
            let head = data.get("head").and_then(Value::as_str).map(str::to_owned);
            landings.push(Landing { card, time_ms, head, before, after });
        }
        Ok(landings)
    }

    /// Terminal journal entries in the half-open window, with the run that ended even if it reopened.
    pub(crate) fn lookback_ends(&self, project: &ProjectId, since: i64, until: i64) -> Result<Vec<Value>> {
        // Older PR approvals could reopen a done review in merging without a card.start row.
        let sql = "SELECT ended.card, ended.time, ended.seq, json_extract(ended.data, '$.stage'),
                       json_extract(started.data, '$.mode'), coalesce(json_extract(ended.data, '$.run'),
                           json_extract(started.data, '$.run') + (
                               SELECT count(*) FROM events AS prior WHERE json_extract(started.data, '$.mode') = 'review'
                                 AND prior.card = ended.card AND prior.kind = 'card.stage'
                                 AND prior.seq > started.seq AND prior.seq < ended.seq AND json_extract(prior.data, '$.stage') = 'done'
                                 AND (SELECT json_extract(next.data, '$.stage') FROM events AS next
                                      WHERE next.card = ended.card AND next.kind = 'card.stage'
                                        AND next.seq > prior.seq AND next.seq < ended.seq
                                      ORDER BY next.seq LIMIT 1) = 'merging'
                           ))
                   FROM events AS ended JOIN entities AS card ON card.kind = 'card' AND card.id = ended.card
                   LEFT JOIN events AS started ON started.seq = (
                       SELECT start.seq FROM events AS start
                       WHERE start.card = ended.card AND start.kind = 'card.start' AND start.seq < ended.seq
                       ORDER BY start.seq DESC LIMIT 1
                   )
                   WHERE ended.kind = 'card.stage' AND json_extract(card.data, '$.project') = ?1
                     AND ended.time >= ?2 AND ended.time < ?3 AND json_extract(ended.data, '$.stage') IN ('done', 'closed')
                   ORDER BY ended.seq";
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params![project.as_str(), since, until])?;
        let mut ends = Vec::new();
        while let Some(row) = rows.next()? {
            let card: String = row.get(0)?;
            let time_ms: i64 = row.get(1)?;
            let seq: i64 = row.get(2)?;
            let stage: String = row.get(3)?;
            let mode: Option<String> = row.get(4)?;
            let run: Option<u32> = row.get(5)?;
            ends.push(json!({"card": card, "ended_ms": time_ms, "seq": seq, "stage": stage, "run": run, "mode": mode}));
        }
        Ok(ends)
    }
}
