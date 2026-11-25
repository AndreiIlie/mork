// One project's hook never reads or forgets another's stream lines (fac-119, fac-244).

use factory_types::orchestrator::OrchestratorEvent;
use factory_types::orchestrator::OrchestratorWhat;
use factory_types::{AskId, ProjectId};
use rusqlite::params;

use super::{Result, Store};

const DELIVERED: &str = "orchestrator_delivered:";

impl Store {
    /// The number of the last line written to the project's hook; none before its first hook ever.
    pub fn delivered(&self, project: &ProjectId) -> Result<Option<u64>> {
        Ok(self.meta(&format!("{DELIVERED}{project}"))?.and_then(|v| v.parse().ok()))
    }

    /// Records the last line written to the project's hook, and forgets its lines up to it.
    pub fn set_delivered(&self, project: &ProjectId, seq: u64) -> Result<()> {
        self.set_meta(&format!("{DELIVERED}{project}"), &seq.to_string())?;
        self.conn.execute(
            "DELETE FROM pa_events WHERE project = ?1 AND seq <= ?2",
            params![project.as_str(), to_sql(seq)],
        )?;
        Ok(())
    }

    /// The project's first hook keeps its eligible `given` and `opinion` lines, and all its `review`
    /// lines. Forgets its others up to `now` and answers the line the hook starts after.
    pub fn first_hook(&self, project: &ProjectId, now: u64, given: &[AskId], opinions: &[AskId]) -> Result<u64> {
        let mut kept = Vec::new();
        for e in self.events_after(project, 0, now)? {
            let keep = match &e.what {
                OrchestratorWhat::Given(a) => given.contains(&a.ask),
                OrchestratorWhat::Opinion { ask, .. } => opinions.contains(&ask.ask),
                OrchestratorWhat::Review { .. } => true,
                _ => false,
            };
            if keep {
                kept.push(e.seq);
            } else {
                self.conn.execute("DELETE FROM pa_events WHERE seq = ?1", params![to_sql(e.seq)])?;
            }
        }
        let from = kept.first().map_or(now, |seq| seq - 1);
        self.set_delivered(project, from)?;
        Ok(from)
    }

    /// At most `limit` of the project's lines after `seq`, oldest first.
    pub fn events_after(&self, project: &ProjectId, seq: u64, limit: u64) -> Result<Vec<OrchestratorEvent>> {
        // A retired step's ask (fac-329, fac-411) is of no kind this build decodes: never counted, never sent.
        let mut stmt = self.conn.prepare(
            "SELECT data FROM pa_events WHERE project = ?1 AND seq > ?2 AND COALESCE(json_extract(data, '$.kind'), json_extract(data, '$.ask.kind'), '') NOT IN ('architecture', 'preview') ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![project.as_str(), to_sql(seq), to_sql(limit)], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for data in rows {
            out.push(serde_json::from_str(&data?)?);
        }
        Ok(out)
    }

    // The next line's number: past every line kept and the last one delivered to any hook, from 1.
    pub(super) fn orchestrator_next(&self) -> Result<u64> {
        let kept: Option<i64> = self.conn.query_row("SELECT MAX(seq) FROM pa_events", [], |r| r.get(0))?;
        let delivered: Option<i64> = self.conn.query_row(
            "SELECT MAX(CAST(value AS INTEGER)) FROM meta WHERE key GLOB ?1",
            params![format!("{DELIVERED}*")],
            |r| r.get(0),
        )?;
        let last = |n: Option<i64>| n.and_then(|n| u64::try_from(n).ok()).unwrap_or(0);
        Ok(last(kept).max(last(delivered)) + 1)
    }
}

pub(super) fn add(tx: &rusqlite::Transaction<'_>, e: &OrchestratorEvent) -> Result<()> {
    tx.execute(
        "INSERT INTO pa_events (seq, project, data) VALUES (?1, ?2, ?3)",
        params![to_sql(e.seq), e.project.as_str(), serde_json::to_string(e)?],
    )?;
    Ok(())
}

fn to_sql(seq: u64) -> i64 {
    i64::try_from(seq).unwrap_or(i64::MAX)
}
