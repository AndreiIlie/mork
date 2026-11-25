//! The state's version (2.3, fac-313): a state before `OLDEST` needs an archived build that reads
//! it; one in `OLDEST..NEWEST` advances at open.

use rusqlite::Transaction;

/// The oldest state version this build reads (fac-658).
pub const OLDEST: u32 = 49;

// 50 fences builds without `gated` (fac-658); 51 without `AskKind::Triage`/`Card::triage` (fac-680); 52 those
// dropping `PendingTriage::stalemate` (fac-694); 53 those whose strict `Hand` refuses its typed fields (fac-696).
/// The newest state version this build writes.
pub const NEWEST: u32 = 53;

const V31: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE jobs (id TEXT PRIMARY KEY, status TEXT NOT NULL, settled_ms INTEGER, data TEXT NOT NULL);
CREATE TABLE actions (id TEXT PRIMARY KEY, natural_key TEXT NOT NULL UNIQUE, status TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT, time INTEGER NOT NULL, card TEXT, kind TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE entities (kind TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY (kind, id));
CREATE TABLE messages (seq INTEGER PRIMARY KEY, place TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE pa_events (seq INTEGER PRIMARY KEY, data TEXT NOT NULL, project TEXT);
CREATE INDEX events_card ON events (card, seq);
CREATE INDEX messages_place ON messages (place, seq);
CREATE INDEX events_kind_time ON events (kind, time);
CREATE INDEX pa_events_project ON pa_events (project, seq);
INSERT INTO meta (key, value) VALUES ('revision', '0');
";

/// Creates the newest version's tables, the baseline's, in an empty database.
pub fn create(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(V31)
}
