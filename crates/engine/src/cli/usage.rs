use mork_types::cost::Tokens;
use serde_json::Value;

use super::StreamState;

const CLAUDE_COUNTS: [&str; 4] = ["inputTokens", "cacheReadInputTokens", "cacheCreationInputTokens", "outputTokens"];
const APP_SERVER_COUNTS: [&str; 4] = ["inputTokens", "cachedInputTokens", "cacheWriteInputTokens", "outputTokens"];

pub(super) fn claude(st: &mut StreamState, ev: &Value) {
    let Some(session) = ev.get("session_id").and_then(Value::as_str) else { return };
    let Some(models) = ev.get("modelUsage").and_then(Value::as_object).filter(|m| !m.is_empty()) else { return };
    let total = models.values().try_fold(Tokens::default(), |total, model| {
        counts(model, CLAUDE_COUNTS).map(|tokens| total.saturating_add(tokens))
    });
    if let Some(total) = total {
        st.token_totals.insert(session.to_owned(), total);
    }
}

pub(super) fn codex_exec(st: &mut StreamState, ev: &Value) {
    let Some(thread) = st.thread.clone() else { return };
    let Some(tokens) = ev.get("usage").and_then(codex_record) else { return };
    st.token_totals.insert(thread, tokens);
}

// Codex counts the cached part in its input total.
pub(super) fn codex_record(usage: &Value) -> Option<Tokens> {
    counts(usage, ["input_tokens", "cached_input_tokens", "cache_write_input_tokens", "output_tokens"]).map(disjoint)
}

pub(super) fn codex_app_server(st: &mut StreamState, params: &Value) {
    let Some(thread) = params.get("threadId").and_then(Value::as_str) else { return };
    let Some(tokens) = params.pointer("/tokenUsage/total").and_then(|u| counts(u, APP_SERVER_COUNTS)) else { return };
    st.token_totals.insert(thread.to_owned(), disjoint(tokens));
}

fn counts(v: &Value, fields: [&str; 4]) -> Option<Tokens> {
    Some(Tokens {
        input: v.get(fields[0])?.as_u64()?,
        cache_read: v.get(fields[1]).map_or(Some(0), Value::as_u64)?,
        cache_write: v.get(fields[2]).map_or(Some(0), Value::as_u64)?,
        output: v.get(fields[3])?.as_u64()?,
    })
}

fn disjoint(tokens: Tokens) -> Tokens {
    Tokens { input: tokens.input.saturating_sub(tokens.cache_read).saturating_sub(tokens.cache_write), ..tokens }
}
