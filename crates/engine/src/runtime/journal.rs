use std::sync::Arc;

use mork_core::views::journal::{Read, scope};
use mork_types::Command;
use mork_types::protocol::{ErrorBody, ErrorCode};
use mork_types::state::Sender;
use serde_json::Value;

use super::Engine;
use crate::sockets::Conn;
use crate::store::Store;

impl Engine {
    pub(super) fn history_journal(
        &self,
        conn: &Arc<Conn>,
        id: u64,
        from: &Sender,
        cmd: Command,
    ) -> Option<Result<Value, ErrorBody>> {
        let project = match scope(&self.state, from, self.ctx.config.mork.project.as_ref()) {
            Ok(p) => p,
            Err(why) => return Some(Err(ErrorBody::new(ErrorCode::Refused, why))),
        };
        let Command::HistoryJournal { card, count, seq, at, before, kinds } = cmd else {
            return Some(Err(ErrorBody::new(ErrorCode::Invalid, "not a journal read")));
        };
        let read = match Read::new(card.as_str(), count, seq, at, before, kinds.unwrap_or_default()) {
            Ok(r) => r,
            Err(why) => return Some(Err(ErrorBody::new(ErrorCode::Invalid, why))),
        };
        let path = self.store.path.clone();
        let conn = Arc::clone(conn);
        std::thread::spawn(move || {
            let answer = Store::open_read_only(&path).and_then(|s| s.history_journal(project.as_ref(), &read));
            let answer = answer.map_err(|e| e.to_string()).and_then(|v| v);
            conn.answer(id, answer.map_err(|why| ErrorBody::new(ErrorCode::Refused, why)));
        });
        None
    }
}
