//! The store (2.1): one SQLite database, `state.db`, with one writer, the engine. WAL mode,
//! `synchronous = FULL`, foreign keys on. The only place `rusqlite` appears (1.12.2).

mod history;
mod journal;
mod journal_read;
mod kept;
mod live;
mod lookback;
mod memory;
mod notes;
mod orchestrators;
mod retired;
pub mod schema;

use std::path::{Path, PathBuf};

use mork_core::decide::Journal;
use mork_core::state::{Change, State};
use mork_types::ask::Ask;
use mork_types::card::Card;
use mork_types::lease::Lease;
use mork_types::state::{Agent, Place};
use mork_types::usage::Limit;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

/// What went wrong opening or writing the state.
#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    /// The state's version isn't one this build reads.
    Version {
        found: u32,
        oldest: u32,
        newest: u32,
    },
    Io(std::io::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "state: {e}"),
            Self::Json(e) => write!(f, "state: a record that isn't valid: {e}"),
            Self::Version { found, oldest, newest } if oldest == newest => {
                write!(f, "state: version {found}, but this build requires version {newest}")
            }
            Self::Version { found, oldest, newest } => {
                write!(f, "state: version {found}, but this build reads {oldest} to {newest}")
            }
            Self::Io(e) => write!(f, "state: {e}"),
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// One event of the journal: time, card, kind, data.
pub type JournalRow = (i64, Option<String>, String, serde_json::Value);

/// The open state.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    pub path: PathBuf,
    pub version: u32,
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// The state's version (`user_version`) of a database file, read-only.
pub fn peek_version(path: &Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(Some(v))
}

fn set_meta_in(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

impl Store {
    /// Opens a supported state for writing or creates one at the newest version (2.1.5).
    /// `build` is the commit `meta` records as creating it (2.2.5).
    pub fn open(path: &Path, build: &str) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        let mut version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let wrong = StoreError::Version { found: version, oldest: schema::OLDEST, newest: schema::NEWEST };
        if version == 0 {
            // A state older than any version: not an empty file to start.
            let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
            if tables > 0 {
                return Err(wrong);
            }
        } else if !(schema::OLDEST..=schema::NEWEST).contains(&version) {
            return Err(wrong);
        }
        configure(&conn)?;
        if version == 0 {
            let tx = conn.transaction()?;
            schema::create(&tx)?;
            set_meta_in(&tx, "created_by", build)?;
            set_meta_in(&tx, "migrated_by", build)?;
            tx.pragma_update(None, "user_version", schema::NEWEST)?;
            tx.commit()?;
            version = schema::NEWEST;
        }
        Ok(Self { conn, path: path.to_owned(), version })
    }

    /// Opens a state read-only, as `mork status` and `mork doctor` do with no app. With no
    /// write-ahead log beside it, it opens immutable, so not even a `-wal` file is made (T2.2).
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let wal =
            path.with_file_name(format!("{}-wal", path.file_name().and_then(|n| n.to_str()).unwrap_or("state.db")));
        let conn = if wal.exists() {
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?
        } else {
            let uri = format!("file:{}?mode=ro&immutable=1", path.display());
            Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)?
        };
        let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        Ok(Self { conn, path: path.to_owned(), version })
    }

    /// Copies the state with `VACUUM INTO` to `backups/state-v<version>-<time>-<why>.db`.
    pub fn backup(&self, dir: &Path, now: i64, why: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(StoreError::Io)?;
        let dest = dir.join(format!("state-v{}-{now}-{why}.db", self.version));
        if dest.exists() {
            std::fs::remove_file(&dest).map_err(StoreError::Io)?;
        }
        self.conn.execute("VACUUM INTO ?1", params![dest.display().to_string()])?;
        Ok(dest)
    }

    /// A pragma's value, for checks (T2.1).
    pub fn pragma(&self, name: &str) -> Result<String> {
        Ok(self.conn.pragma_query_value(None, name, |r| r.get::<_, rusqlite::types::Value>(0)).map(|v| match v {
            rusqlite::types::Value::Integer(i) => i.to_string(),
            rusqlite::types::Value::Text(t) => t,
            rusqlite::types::Value::Null | rusqlite::types::Value::Real(_) | rusqlite::types::Value::Blob(_) => {
                String::new()
            }
        })?)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0)).optional()?)
    }

    /// Sets a meta value outside any change: a backup's or a prune's time. It doesn't
    /// raise the revision (2.1.4).
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn revision(&self) -> Result<u64> {
        Ok(self.meta("revision")?.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    fn rows(&self, sql: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| {
            let a: rusqlite::types::Value = r.get(0)?;
            let key = match a {
                rusqlite::types::Value::Text(t) => t,
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Null | rusqlite::types::Value::Real(_) | rusqlite::types::Value::Blob(_) => {
                    String::new()
                }
            };
            Ok((key, r.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Commits one transaction (2.1.4, 2.4.1), raising the revision by one when anything changed.
    pub fn commit(&mut self, changes: &[Change], journal: &[Journal], now: i64) -> Result<()> {
        if !changes.iter().any(Change::is_stored) && journal.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for c in changes {
            write_change(&tx, c)?;
            kept::change(&tx, c)?;
        }
        for j in journal {
            kept::journal(&tx, j)?;
            tx.execute(
                "INSERT INTO events (time, card, kind, data) VALUES (?1, ?2, ?3, ?4)",
                params![now, j.card, j.kind, j.data.to_string()],
            )?;
        }
        if changes.iter().any(Change::is_stored) {
            tx.execute("UPDATE meta SET value = CAST(value AS INTEGER) + 1 WHERE key = 'revision'", [])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The jobs settled before `t`, whose folders go (fac-336: ended cards' too).
    pub fn settled_before(&self, t: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM jobs WHERE status = 'settled' AND settled_ms < ?1")?;
        Ok(stmt.query_map([t], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    /// A job's kind and settling time, read without its record (`status --jobs --all`).
    pub fn job_seen(&self, id: &str) -> Result<Option<(String, Option<i64>)>> {
        let sql = "SELECT json_extract(data, '$.kind'), status, settled_ms FROM jobs WHERE id = ?1";
        let mut stmt = self.conn.prepare_cached(sql)?;
        let row =
            stmt.query_row([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)));
        Ok(row.optional()?.map(|(kind, status, ms)| (kind, ms.filter(|_| status == "settled"))))
    }

    /// Every job's id, with its settling time once settled: doctor's, which knows ended cards' jobs too (fac-336).
    pub fn jobs_seen(&self) -> Result<Vec<(String, Option<i64>)>> {
        let mut stmt = self.conn.prepare("SELECT id, status, settled_ms FROM jobs")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?)))?;
        let mut jobs = Vec::new();
        for row in rows {
            let (id, status, settled_ms) = row?;
            jobs.push((id, settled_ms.filter(|_| status == "settled")));
        }
        Ok(jobs)
    }

    /// The latest journal event of a kind, if there is one: its time, and its data.
    pub fn last_journaled(&self, kind: &str) -> Result<Option<(i64, serde_json::Value)>> {
        let row: Option<(i64, String)> = self
            .conn
            .query_row("SELECT time, data FROM events WHERE kind = ?1 ORDER BY seq DESC LIMIT 1", params![kind], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        Ok(row.and_then(|(t, d)| Some((t, serde_json::from_str(&d).ok()?))))
    }
}

fn load_entity(s: &mut State, kind: &str, data: &str) -> Result<()> {
    match kind {
        "agent" => {
            let v: serde_json::Value = serde_json::from_str(data)?;
            if !retired::mocker(&v) {
                let a: Agent = serde_json::from_value(v)?;
                s.agents.insert(a.key.clone(), a);
            }
        }
        "place" => {
            let p: Place = serde_json::from_str(data)?;
            s.places.insert(p.id.clone(), p);
        }
        "card" => {
            let c: Card = serde_json::from_str(data)?;
            s.cards.insert(c.id.clone(), c);
        }
        "ask" => {
            let v: serde_json::Value = serde_json::from_str(data)?;
            if !retired::step_ask(&v) {
                let a: Ask = serde_json::from_value(v)?;
                s.asks.insert(a.id.clone(), a);
            }
        }
        "limit" => {
            let l: Limit = serde_json::from_str(data)?;
            s.limits.insert(l.key(), l);
        }
        "lease" => {
            let l: Lease = serde_json::from_str(data)?;
            s.leases.insert(l.id.clone(), l);
        }
        "note" | "note-last" => notes::load(s, kind, data)?,
        "pass" => s.passes.push(serde_json::from_str(data)?),
        "auto" => {
            let a: mork_types::orchestrator::Auto = serde_json::from_str(data)?;
            s.autos.insert(a.project.clone(), a);
        }
        "lookback" => {
            let c: mork_types::lookback::Cursor = serde_json::from_str(data)?;
            s.lookbacks.entry(c.project.clone()).or_default().insert(c.read, c);
        }
        "forge-token" => {
            let (project, why) = serde_json::from_str(data)?;
            s.world.token_refused.insert(project, why);
        }
        "memory-file" | "memory-tally" => memory::load(s, kind, data)?,
        "owed-work" => {
            let v: serde_json::Value = serde_json::from_str(data)?;
            if !retired::research_purpose(&v) {
                let w: mork_types::work::Purpose = serde_json::from_value(v)?;
                s.owed_work.insert(w.key(), w);
            }
        }
        _ => {}
    }
    Ok(())
}

fn write_token(tx: &rusqlite::Transaction<'_>, project: &mork_types::ProjectId, refused: Option<&str>) -> Result<()> {
    if let Some(words) = refused {
        put_entity(tx, "forge-token", project, &serde_json::to_string(&(project, words))?)?;
    } else {
        tx.execute("DELETE FROM entities WHERE kind = 'forge-token' AND id = ?1", params![project.as_str()])?;
    }
    Ok(())
}

fn write_change(tx: &rusqlite::Transaction<'_>, c: &Change) -> Result<()> {
    match c {
        Change::PutAgent(a) => put_entity(tx, "agent", a.key.as_str(), &serde_json::to_string(a)?)?,
        Change::RemoveAgent { key } => {
            tx.execute("DELETE FROM entities WHERE kind = 'agent' AND id = ?1", params![key.as_str()])?;
        }
        Change::PutPlace(p) => put_entity(tx, "place", &p.id, &serde_json::to_string(p)?)?,
        Change::PutJob(j) => {
            let status = serde_json::to_value(&j.status)?.as_str().unwrap_or("").to_owned();
            tx.execute(
                "INSERT OR REPLACE INTO jobs (id, status, settled_ms, data) VALUES (?1, ?2, ?3, ?4)",
                params![j.id.as_str(), status, j.settled_ms, serde_json::to_string(j)?],
            )?;
        }
        Change::PutCard(c) => {
            retired::drop_archived_builders(tx, c)?;
            put_entity(tx, "card", c.id.as_str(), &serde_json::to_string(c)?)?;
        }
        Change::PutAsk(a) => put_entity(tx, "ask", &a.id, &serde_json::to_string(a)?)?,
        Change::PutLimit(l) => put_entity(tx, "limit", &l.key(), &serde_json::to_string(l)?)?,
        Change::PutAction(a) => {
            let status = serde_json::to_value(a.status)?.as_str().unwrap_or("").to_owned();
            tx.execute(
                "INSERT OR REPLACE INTO actions (id, natural_key, status, data) VALUES (?1, ?2, ?3, ?4)",
                params![a.id.as_str(), a.key, status, serde_json::to_string(a)?],
            )?;
        }
        Change::PutLease(l) => put_entity(tx, "lease", &l.id, &serde_json::to_string(l)?)?,
        Change::RemoveLease { id } => {
            tx.execute("DELETE FROM entities WHERE kind = 'lease' AND id = ?1", params![id.as_str()])?;
        }
        Change::AddPass(p) => {
            let id = format!("{}@{}@{}", p.project, p.tree, p.command.join(" "));
            put_entity(tx, "pass", &id, &serde_json::to_string(p)?)?;
        }
        Change::PutAuto(a) => put_entity(tx, "auto", a.project.as_str(), &serde_json::to_string(a)?)?,
        Change::PutLookback(c) => {
            let id = format!("{}/{}", c.project, c.read.spec().name);
            put_entity(tx, "lookback", &id, &serde_json::to_string(c)?)?;
        }
        Change::PutOwedWork(w) => put_entity(tx, "owed-work", &w.key(), &serde_json::to_string(w)?)?,
        Change::RemoveOwedWork { key } => {
            tx.execute("DELETE FROM entities WHERE kind = 'owed-work' AND id = ?1", params![key])?;
        }
        Change::PutMemoryFile(_) | Change::RemoveMemoryFile { .. } | Change::PutMemoryTally(_) => {
            memory::write(tx, c)?;
        }
        // The token's verdict outlives a restart (3.7.2, M3.2).
        Change::SetTokenRefused { project, refused } => write_token(tx, project, refused.as_deref())?,
        Change::SetBoard { .. } | Change::SetOpenPrs { .. } | Change::SetChecksFolder { .. } => {}
        Change::AddOrchestratorEvent(e) => orchestrators::add(tx, e)?,
        Change::PutNote(_) | Change::RemoveNote { .. } => notes::write(tx, c)?,
        Change::AddMessage(m) => {
            tx.execute(
                "INSERT INTO messages (seq, place, data) VALUES (?1, ?2, ?3)",
                params![i64::try_from(m.seq).unwrap_or(i64::MAX), m.place.as_str(), serde_json::to_string(m)?],
            )?;
        }
    }
    Ok(())
}

fn put_entity(tx: &rusqlite::Transaction<'_>, kind: &str, id: &str, data: &str) -> Result<()> {
    tx.execute("INSERT OR REPLACE INTO entities (kind, id, data) VALUES (?1, ?2, ?3)", params![kind, id, data])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Store, StoreError, schema};

    #[test]
    // why: a failed database operation fails this test.
    #[allow(clippy::unwrap_used)]
    fn pre_cutover_state_can_be_inspected_but_requires_upgrade_for_writes() {
        let dir = std::env::temp_dir().join(format!("mork-state-version-{}", crate::random::uuid()));
        std::fs::create_dir(&dir).unwrap();
        let db = dir.join("state.db");
        let store = Store::open(&db, "test").unwrap();
        assert_eq!(store.version, schema::NEWEST);
        store.conn.pragma_update(None, "user_version", 54).unwrap();
        drop(store);

        assert!(matches!(Store::open(&db, "test"), Err(StoreError::Version { found: 54, oldest: 55, .. })));
        assert_eq!(Store::open_read_only(&db).unwrap().version, 54);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
