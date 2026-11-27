use super::{Result, Store};
use mork_core::routing::{WINDOW, observe};
use mork_types::routing::Statistics;

const ROWS:&str = "WITH recent AS (
    SELECT DISTINCT card FROM events WHERE kind IN ('merge.tested','review.verdict','card.landed','card.stage','cost','route') AND time>=?1 AND card IS NOT NULL
), links AS (
    SELECT card,json_extract(data,'$.of') AS owner FROM events
    WHERE kind IN ('card.start','review.adopted','review.ended') AND json_extract(data,'$.of') IS NOT NULL
), subjects AS (
    SELECT card FROM recent UNION SELECT owner FROM links WHERE card IN recent UNION SELECT card FROM links WHERE owner IN recent
)
SELECT seq,time,card,kind,data FROM events INDEXED BY events_card WHERE card IN subjects AND kind IN
('card.start','card.stage','cost','card.landed','merge.tested','reply','review.adopted','review.ended',
'child.spawned','review.asked','review.verdict','review.resolved','review.findings.withdrawn','build.requested',
'build.submitted','brief.approved','card.rebriefed','challenge.answered','lookback.done','route') ORDER BY seq";

impl Store {
    pub fn routing(&self, now: i64) -> Result<Statistics> {
        let tx = self.conn.unchecked_transaction()?;
        let histories = super::quality::histories(&tx, now.saturating_sub(WINDOW), ROWS)?;
        tx.commit()?;
        Ok(observe::aggregate(now, &histories))
    }
}
