//! What a turn's stream said beside its steps (fac-96): its text and thinking, each once, with
//! the next step of its thread, within bounds.

use std::collections::{HashMap, HashSet};

use mork_types::JobId;
use mork_types::seen::{CommandView, Detail, DetailKind, ReadPoint, StepView, TurnView};

use super::{Step, StreamState};
use crate::files::sha256_hex;

/// A detail's text past this is cut.
pub const DETAIL_MAX: usize = 16 * 1024;
/// What a stream keeps of its details in all: past it, the oldest steps' go first.
pub const DETAILS_MAX: usize = 512 * 1024;

/// Text or thinking the stream gave, once per `key`, which its id is of: it waits for the next
/// step of its thread (a subagent's, by its tool call, or the turn's own); empty text is none.
pub fn detail(st: &mut StreamState, key: &str, thread: Option<&str>, kind: DetailKind, text: &str) {
    let text = text.trim();
    let id = sha256_hex(key.as_bytes())[..16].to_owned();
    if text.is_empty() || !st.detail_keys.insert(id.clone()) {
        return;
    }
    let (text, cut) = cut_to(text, DETAIL_MAX);
    let d = Detail { id, kind, text, cut, sub: thread.is_some() };
    st.pending.entry(thread.map(str::to_owned)).or_default().push(d);
    keep_within(st);
}

/// A step of `thread`, with what waited for it there, started when the line in hand was read.
pub fn step(st: &mut StreamState, thread: Option<&str>, id: &str, words: String, command: Option<CommandView>) {
    let thread = thread.map(str::to_owned);
    let details = st.pending.remove(&thread).unwrap_or_default();
    st.steps.push(Step { id: id.to_owned(), words, command, details, thread, start_ms: st.at, end_ms: None });
}

/// The step `id` ended when the line in hand was read; its first end holds.
pub fn ended(st: &mut StreamState, id: &str) {
    let at = st.at;
    if let Some(s) = st.steps.iter_mut().find(|s| s.id == id && s.end_ms.is_none()) {
        s.end_ms = at;
    }
}

/// The turn started when the line in hand was read; the first time holds.
pub fn started(st: &mut StreamState) {
    st.started_ms = st.started_ms.or(st.at);
}

/// The turn ended when the line in hand was read; the first time holds.
pub fn finished(st: &mut StreamState) {
    st.ended_ms = st.ended_ms.or(st.at);
}

/// The job ended at `last`, as the engine saw it: a step it left running, as a cut turn's, ended
/// then, or at the turn's end if later.
pub fn close(st: &mut StreamState, last: Option<i64>) {
    let at = last.max(st.ended_ms);
    for s in st.steps.iter_mut().filter(|s| s.end_ms.is_none()) {
        s.end_ms = s.start_ms.zip(at).map(|(start, end)| start.max(end));
    }
}

/// Records each newly read message before the next step of the turn's own.
pub fn read(st: &mut StreamState) {
    let step = st.steps.iter().filter(|s| s.thread.is_none()).count();
    for id in &st.taken {
        st.read_at.entry(id.clone()).or_insert(step);
    }
}

/// The turn as its job's stream told it. What no step of the turn's own followed closes it; a
/// subagent's last words go with the step that ran it.
pub fn turn_of(st: &StreamState, job: &JobId) -> TurnView {
    let mut steps: Vec<StepView> = st
        .steps
        .iter()
        .map(|s| StepView {
            id: s.id.clone(),
            words: s.words.clone(),
            command: s.command.clone(),
            details: s.details.clone(),
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            thought_ms: None,
        })
        .collect();
    let mut after = Vec::new();
    for (thread, details) in &st.pending {
        match thread.as_ref().and_then(|t| steps.iter_mut().find(|s| s.id == *t)) {
            Some(s) => s.details.extend(details.iter().cloned()),
            None => after.extend(details.iter().cloned()),
        }
    }
    let mut t = TurnView { job: job.clone(), steps, after, cut: st.details_cut, ..TurnView::default() };
    thinking(st, &mut t);
    t.steps = beneath(st, t.steps);
    t.read = read_points(st, &t);
    t
}

fn read_points(st: &StreamState, t: &TurnView) -> Vec<ReadPoint> {
    let own: Vec<_> = st.steps.iter().filter(|s| s.thread.is_none()).collect();
    let mut read: Vec<_> = st
        .read_at
        .iter()
        .filter(|(id, _)| st.taken.contains(id))
        .filter_map(|(id, step)| {
            let seq = super::live::seq_of(t.job.as_str(), id)?;
            let next = own.get(*step).and_then(|s| t.steps.iter().position(|v| v.id == s.id));
            Some(ReadPoint { seq, step: next.unwrap_or(t.steps.len()) })
        })
        .collect();
    read.sort_by_key(|r| (r.step, r.seq));
    read
}

// A subagent's steps go beneath the step that ran it, each followed by its own subagents' before
// its next sibling, however the turn's own interleave with them (fac-206).
fn beneath(st: &StreamState, steps: Vec<StepView>) -> Vec<StepView> {
    let ids: HashSet<&str> = st.steps.iter().map(|s| s.id.as_str()).collect();
    let mut under: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut tops = Vec::new();
    for (i, s) in st.steps.iter().enumerate() {
        match s.thread.as_deref().filter(|t| ids.contains(t)) {
            Some(t) => under.entry(t).or_default().push(i),
            None => tops.push(i),
        }
    }
    let mut slots: Vec<Option<StepView>> = steps.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(slots.len());
    // Steps whose threads run in a circle reach no top: they follow, in the stream's order.
    let mut stack: Vec<usize> = tops.into_iter().rev().collect();
    let mut rest = (0..slots.len()).rev().collect::<Vec<_>>();
    while let Some(i) = stack.pop().or_else(|| rest.pop()) {
        let Some(v) = slots.get_mut(i).and_then(Option::take) else { continue };
        if let Some(sub) = st.steps.get(i).and_then(|s| under.remove(s.id.as_str())) {
            stack.extend(sub.into_iter().rev());
        }
        out.push(v);
    }
    out
}

/// A gap this long or longer with no tool running is the model thinking (fac-206).
pub const THOUGHT_MIN_MS: i64 = 1000;

// Every step's run is busy time, a subagent's too, which may run on after the call that started it
// returned. `free` is when no tool ran since, `i64::MAX` while one runs; none when unknown.
fn thinking(st: &StreamState, t: &mut TurnView) {
    let mut free = st.started_ms;
    for (s, v) in st.steps.iter().zip(t.steps.iter_mut()) {
        if s.thread.is_none() {
            v.thought_ms = gap(free, s.start_ms);
        }
        let end = s.end_ms.or(s.start_ms.map(|_| i64::MAX));
        free = end.map(|e| free.map_or(e, |f| f.max(e)));
    }
    match st.ended_ms {
        Some(_) => t.thought_ms = gap(free, st.ended_ms),
        None => t.thinking_since = free.filter(|f| *f != i64::MAX),
    }
}

fn gap(free: Option<i64>, until: Option<i64>) -> Option<i64> {
    let ms = until?.checked_sub(free?)?;
    (ms >= THOUGHT_MIN_MS).then_some(ms)
}

pub(crate) fn cut_to(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

// Past `DETAILS_MAX`, the oldest details go: the earliest steps' first, then the earliest waiting.
fn keep_within(st: &mut StreamState) {
    let size = |d: &[Detail]| d.iter().map(|x| x.text.len()).sum::<usize>();
    let mut total =
        st.steps.iter().map(|s| size(&s.details)).sum::<usize>() + st.pending.values().map(|d| size(d)).sum::<usize>();
    if total <= DETAILS_MAX {
        return;
    }
    st.details_cut = true;
    let waiting = st.pending.values_mut();
    for details in st.steps.iter_mut().map(|s| &mut s.details).chain(waiting) {
        while total > DETAILS_MAX && !details.is_empty() {
            total -= details.remove(0).text.len();
        }
    }
}
