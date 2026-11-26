// `history.search` (fac-371): a scan of every card's history, so it runs on a thread of its own
// with its own read-only connection, holding up no other command, view or turn.

use std::sync::Arc;

use mork_core::views::search::{Search, on_boards, scope};
use mork_types::protocol::{ErrorBody, ErrorCode};
use mork_types::state::Sender;
use serde_json::Value;

use super::Engine;
use crate::sockets::Conn;
use crate::store::Store;

impl Engine {
    pub(super) fn history_search(
        &self,
        conn: &Arc<Conn>,
        id: u64,
        from: &Sender,
        words: String,
    ) -> Option<Result<Value, ErrorBody>> {
        let project = match scope(&self.state, from) {
            Ok(p) => p,
            Err(why) => return Some(Err(ErrorBody::new(ErrorCode::Refused, why))),
        };
        // The boards as last read, so an ended card's notes are as its file now has them.
        let boards = on_boards(&self.state, project.as_ref());
        let path = self.store.path.clone();
        let conn = Arc::clone(conn);
        std::thread::spawn(move || {
            let answer = Store::open_read_only(&path).and_then(|s| {
                let mut search = Search::new(&words, boards, &s.board_writes(project.as_ref())?);
                s.history(project.as_ref(), |f| search.offer(f))?;
                Ok(search.answer())
            });
            conn.answer(id, answer.map_err(|e| ErrorBody::new(ErrorCode::Refused, e.to_string())));
        });
        None
    }
}
