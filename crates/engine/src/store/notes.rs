use mork_core::state::{Change, State};
use mork_types::note::Note;
use rusqlite::Transaction;

use super::{Result, put_entity};

pub fn load(s: &mut State, kind: &str, data: &str) -> Result<()> {
    if kind == "note-last" {
        s.notes_last = s.notes_last.max(serde_json::from_str(data)?);
    } else {
        let n: Note = serde_json::from_str(data)?;
        s.apply(&Change::PutNote(Box::new(n)));
    }
    Ok(())
}

pub fn write(tx: &Transaction<'_>, c: &Change) -> Result<()> {
    if let Change::PutNote(n) = c {
        put_entity(tx, "note", &n.id.to_string(), &serde_json::to_string(n)?)?;
        put_entity(tx, "note-last", "id", &n.id.to_string())?;
    } else if let Change::RemoveNote { id } = c {
        tx.execute("DELETE FROM entities WHERE kind = 'note' AND id = ?1", [id.to_string()])?;
    }
    Ok(())
}
