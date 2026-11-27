use super::{Engine, Input};
use crate::{clock, sockets::Conn, store::Store};
use mork_core::routing::{choose::Inputs, report};
use mork_types::{
    config::Config,
    protocol::{ErrorBody, ErrorCode},
    routing::Statistics,
};
use serde_json::Value;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Cache {
    pub stats: Option<Box<Statistics>>,
    next_read: Option<i64>,
    reading: bool,
}

const REFRESH_MS: i64 = 600_000;

impl Cache {
    pub fn open(config: &Config, store: &Store) -> Result<Self, String> {
        let last = clock::now_ms();
        let stats = config.routing_enabled().then(|| read(&store.path, last)).transpose()?;
        let next_read = stats.as_ref().map(|_| last + REFRESH_MS);
        Ok(Self { stats, next_read, reading: false })
    }

    pub fn configure(&mut self, enabled: bool) {
        if enabled {
            self.next_read.get_or_insert(0);
        } else {
            self.next_read = None;
            self.stats = None;
        }
    }
}

fn read(path: &std::path::Path, now: i64) -> Result<Box<Statistics>, String> {
    Store::open_read_only(path).and_then(|s| s.routing(now)).map(Box::new).map_err(|e| e.to_string())
}

impl Engine {
    pub(super) fn refresh_routing(&mut self, now: i64) {
        if self.routing.reading || self.routing.next_read.is_none_or(|next| now < next) {
            return;
        }
        self.routing.reading = true;
        self.routing.next_read = Some(now + REFRESH_MS);
        let path = self.store.path.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Input::Routing(read(&path, now)));
        });
    }

    pub(super) fn routing_read(&mut self, result: Result<Box<Statistics>, String>) {
        self.routing.reading = false;
        if self.routing.next_read.is_none() {
            return;
        }
        match result {
            Ok(stats) => self.routing.stats = Some(stats),
            Err(e) => self.log(&format!("routing statistics: {e}")),
        }
    }

    pub(super) fn routing_answer(&self, conn: &Arc<Conn>, id: u64) -> Option<Result<Value, ErrorBody>> {
        let now = clock::now_ms();
        let ctx = self.decision_ctx(now);
        if let Some(stats) = ctx.routing {
            return Some(Ok(report::answer(ctx.config, &Inputs::new(&self.state.limits, ctx), stats)));
        }
        let config = ctx.config.clone();
        let account = ctx.claude_account.map(str::to_owned);
        let limits = self.state.limits.clone();
        let path = self.store.path.clone();
        let conn = conn.clone();
        std::thread::spawn(move || {
            let inputs = Inputs { limits: &limits, now, account: account.as_deref(), stats: None };
            let answer = read(&path, now)
                .map(|s| report::answer(&config, &inputs, &s))
                .map_err(|e| ErrorBody::new(ErrorCode::Refused, e));
            conn.answer(id, answer);
        });
        None
    }
}
