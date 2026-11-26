// What `history.search` reads (fac-371), row by row on a connection of its own: the cards in
// scope, their places' posted messages and their journal events, but the messages' own lines.

use std::collections::BTreeSet;

use mork_core::views::search::Found;
use mork_types::ProjectId;
use mork_types::action::{Action, ActionWhat};
use mork_types::card::Card;

use super::{Result, Store};

impl Store {
    /// Gives `each` every card of `project` (every project's with none), its messages and its journal events.
    pub fn history(&self, project: Option<&ProjectId>, mut each: impl FnMut(Found)) -> Result<()> {
        let mut cards = BTreeSet::new();
        let mut stmt = self.conn.prepare("SELECT data FROM entities WHERE kind = 'card'")?;
        for data in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let c: Card = serde_json::from_str(&data?)?;
            if project.is_none_or(|p| *p == c.project) {
                cards.insert(c.id.as_str().to_owned());
                each(Found::Card(Box::new(c)));
            }
        }
        let of = |id: &str| id.split('/').next().filter(|c| cards.contains(*c)).map(str::to_owned);
        let mut stmt = self.conn.prepare("SELECT place, data FROM messages")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            if let Some(card) = of(&r.get::<_, String>(0)?) {
                each(Found::Message { card, message: Box::new(serde_json::from_str(&r.get::<_, String>(1)?)?) });
            }
        }
        let mut stmt = self.conn.prepare("SELECT time, card, kind, data FROM events WHERE kind <> 'message'")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            if let Some(card) = r.get::<_, Option<String>>(1)?.as_deref().and_then(of) {
                let data = serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
                each(Found::Event { card, time: r.get(0)?, kind: r.get(2)?, data });
            }
        }
        Ok(())
    }

    /// The recorded board writes of `project`'s cards (every project's with none), ended cards' included:
    /// what tells their notes apart.
    pub fn board_writes(&self, project: Option<&ProjectId>) -> Result<Vec<Action>> {
        let mut stmt = self.conn.prepare("SELECT data FROM actions WHERE natural_key LIKE 'change-card:%'")?;
        let mut writes = Vec::new();
        for data in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let a: Action = serde_json::from_str(&data?)?;
            let ActionWhat::ChangeBoardCard { project: p, .. } = &a.what else { continue };
            if project.is_none_or(|want| want == p) {
                writes.push(a);
            }
        }
        Ok(writes)
    }
}
