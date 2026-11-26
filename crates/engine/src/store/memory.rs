use mork_core::state::{Change, State};
use mork_types::memory::{MemoryFile, Tally};
use rusqlite::params;

use super::{Result, put_entity};

pub(super) fn load(s: &mut State, kind: &str, data: &str) -> Result<()> {
    match kind {
        "memory-file" => {
            let f: MemoryFile = serde_json::from_str(data)?;
            s.memory_files.insert(f.key(), f);
        }
        "memory-tally" => {
            let t: Tally = serde_json::from_str(data)?;
            s.memory_tallies.insert(t.memory.clone(), t);
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn write(tx: &rusqlite::Transaction<'_>, c: &Change) -> Result<()> {
    match c {
        Change::PutMemoryFile(f) => put_entity(tx, "memory-file", &f.key(), &serde_json::to_string(f)?)?,
        Change::RemoveMemoryFile { key } => {
            tx.execute("DELETE FROM entities WHERE kind = 'memory-file' AND id = ?1", params![key])?;
        }
        Change::PutMemoryTally(t) => put_entity(tx, "memory-tally", &t.memory, &serde_json::to_string(t)?)?,
        _ => {}
    }
    Ok(())
}
