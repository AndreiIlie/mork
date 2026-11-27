use std::collections::BTreeMap;

use mork_core::views::quality::{self, Event, History};
use mork_types::quality::Quality;
use rusqlite::params;

use super::{Result, Store};

// A landed run can start before the window and incur closing costs afterward.
// Read its history by card: probing the kind index here scans years of unrelated turns on every refresh.
const HISTORY: &str = "WITH subjects AS (
    SELECT DISTINCT card FROM events WHERE kind IN ('card.landed','merge.tested') AND time>=?1
), histories AS (
    SELECT card FROM subjects
    UNION SELECT card FROM events WHERE kind='review.verdict' AND time>=?1
    UNION SELECT card FROM events WHERE kind IN ('card.start','review.adopted','review.ended')
        AND json_extract(data,'$.of') IN subjects
)
SELECT seq,time,card,kind,data FROM events INDEXED BY events_card WHERE card IN histories AND kind IN
 ('card.start','card.stage','cost','card.landed','merge.tested','reply','review.adopted','review.ended',
 'child.spawned','review.asked','review.verdict','review.resolved','review.findings.withdrawn',
 'build.requested','build.submitted')
UNION ALL SELECT seq,time,card,kind,data FROM events WHERE kind='review.stalemate' AND time>=?1
UNION ALL SELECT seq,time,card,kind,data FROM events WHERE kind='cost' AND time>=?1
 AND card IS NOT NULL AND card NOT IN (SELECT card FROM histories WHERE card IS NOT NULL)
ORDER BY seq";

impl Store {
    /// Quality metrics from the journal and only those retained reviews needed to interpret older verdicts.
    pub fn quality(&self, now: i64) -> Result<Quality> {
        let since = now.saturating_sub(quality::WINDOW_MS);
        let mut cards = BTreeMap::<String, History>::new();
        let mut query = self.conn.prepare(HISTORY)?;
        let mut rows = query.query([since])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(2)?;
            let event = Event {
                seq: row.get(0)?,
                time: row.get(1)?,
                kind: row.get(3)?,
                data: serde_json::from_str(&row.get::<_, String>(4)?)?,
            };
            cards.entry(id).or_default().rows.push(event);
        }
        let mut meta = self.conn.prepare(
            "SELECT json_extract(data,'$.project'),json_extract(data,'$.mode'),
            CASE WHEN ?2 THEN json_extract(data,'$.review') END FROM entities WHERE kind='card' AND id=?1",
        )?;
        for (id, card) in &mut cards {
            let legacy = card
                .rows
                .iter()
                .any(|e| e.kind == "review.verdict" && e.time >= since && e.data.get("named").is_none());
            let mut result = meta.query(params![id, legacy])?;
            if let Some(row) = result.next()? {
                card.project = row.get(0)?;
                card.mode = row.get(1)?;
                card.legacy = row.get::<_, Option<String>>(2)?.map(|raw| serde_json::from_str(&raw)).transpose()?;
            } else {
                card.project = "unknown".into();
            }
        }
        Ok(quality::aggregate(since, &cards))
    }
}
