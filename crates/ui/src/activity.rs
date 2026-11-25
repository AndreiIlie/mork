// fac-138, fac-206: every line of a job's block shows, never folded, each call with its time.

use std::collections::{BTreeMap, BTreeSet};

use factory_types::chat::ChatTurn;
use factory_types::chat::took;
use factory_types::seen::{Detail, DetailKind, TurnView};
use factory_types::view::MessageView;
use iced::widget::{Column, container, responsive};
use iced::{Element, Length, Padding};

use crate::app::{App, Message};
use crate::spans::Tone;
use crate::titles::Titles;
use crate::widgets::{self, clickable, txt};

pub(crate) enum Entry<'a> {
    Message(&'a MessageView),
    Turn(&'a ChatTurn),
}

enum Row<'a> {
    Words(String),
    Message(&'a MessageView),
}

pub(crate) fn entries<'a>(messages: &'a [MessageView], turns: &'a [ChatTurn]) -> Vec<Entry<'a>> {
    let read: BTreeSet<_> = turns.iter().flat_map(|t| t.turn.read.iter().map(|r| r.seq)).collect();
    let mut turns: Vec<_> = turns.iter().collect();
    turns.sort_by_key(|t| t.after_seq);
    let mut turns = turns.into_iter().peekable();
    let mut entries = Vec::new();
    for m in messages {
        while turns.peek().is_some_and(|t| t.after_seq < m.seq) {
            if let Some(t) = turns.next() {
                entries.push(Entry::Turn(t));
            }
        }
        if !read.contains(&m.seq) {
            entries.push(Entry::Message(m));
        }
    }
    entries.extend(turns.map(Entry::Turn));
    entries
}

pub(crate) fn view<'a>(
    app: &'a App,
    t: &ChatTurn,
    messages: &'a [MessageView],
    titles: &Titles,
) -> Element<'a, Message> {
    let job = t.turn.job.to_string();
    let live = if t.running { " · running" } else { "" };
    let plural = if t.step_count == 1 { "" } else { "s" };
    let head = format!("{} · tools · {} step{plural} · job {job}{live}", t.agent, t.step_count);
    let head = line(head, if t.running { Tone::Green } else { Tone::Dim });
    let mut col = Column::new().push(clickable(format!("activity:{job}"), head, None));
    if t.turn.cut {
        col = col.push(line("earlier steps or texts were left out, to keep within bounds".into(), Tone::Orange));
    }
    let now = t.running.then_some(app.now_ms);
    for row in lines(&t.turn, messages, now) {
        let words = match row {
            Row::Words(words) => words,
            Row::Message(m) => {
                col = col.push(crate::conversation::message(app, titles, m, crate::shape::shape(m)));
                continue;
            }
        };
        // A subagent's lines sit under the call that started it.
        let indent = if words.starts_with('↳') { 4.0 } else { 2.0 };
        let pad = Padding::ZERO.left(crate::fonts::ADVANCE * indent);
        col = col.push(container(line(words, Tone::Dim)).padding(pad));
    }
    col.into()
}

fn line(words: String, tone: Tone) -> Element<'static, Message> {
    responsive(move |size| txt(widgets::cut(&words, widgets::chars_in(size.width)), tone).into())
        .height(Length::Fixed(crate::fonts::LINE))
        .into()
}

fn detail(d: &Detail) -> String {
    let prefix = if d.sub { "↳ " } else { "" };
    let text = factory_types::chat::line(&crate::text::clean(&d.text));
    format!("{prefix}{text}")
}

// `now` while the turn runs: a call still running counts up to it, and so does the model's work
// with no tool running, from its first second.
fn lines<'a>(t: &TurnView, messages: &'a [MessageView], now: Option<i64>) -> Vec<Row<'a>> {
    let thought = |ms: Option<i64>| ms.map(|ms| Row::Words(format!("Thought for {}", took(ms))));
    let by_seq: BTreeMap<_, _> = messages.iter().map(|m| (m.seq, m)).collect();
    let mut read: BTreeMap<usize, Vec<&MessageView>> = BTreeMap::new();
    for point in &t.read {
        if let Some(&m) = by_seq.get(&point.seq) {
            read.entry(point.step.min(t.steps.len())).or_default().push(m);
        }
    }
    let mut lines = Vec::new();
    for i in 0..=t.steps.len() {
        lines.extend(read.remove(&i).unwrap_or_default().into_iter().map(Row::Message));
        let Some(s) = t.steps.get(i) else { continue };
        lines.extend(thought(s.thought_ms));
        lines.extend(s.details.iter().filter(|d| d.kind == DetailKind::Text).map(|d| Row::Words(detail(d))));
        let words = factory_types::chat::line(&crate::text::clean(&s.words));
        let time = match (s.start_ms, s.end_ms.or(now)) {
            (Some(start), Some(end)) => format!(" · {}", took(end - start)),
            _ => String::new(),
        };
        lines.push(Row::Words(format!("{words}{time}")));
    }
    lines.extend(thought(t.thought_ms));
    lines.extend(t.after.iter().filter(|d| d.kind == DetailKind::Text).map(|d| Row::Words(detail(d))));
    let thinking = t.thinking_since.zip(now).map(|(since, now)| now - since);
    lines.extend(thinking.filter(|ms| *ms >= 1000).map(|ms| Row::Words(format!("Thinking… {}", took(ms)))));
    lines
}

pub(crate) fn merge(older: Vec<ChatTurn>, newer: &mut Vec<ChatTurn>, before: Option<u64>) {
    let kept = older.into_iter().filter(|t| before.is_none_or(|b| t.after_seq < b));
    for t in kept {
        if !newer.iter().any(|n| n.turn.job == t.turn.job) {
            newer.push(t);
        }
    }
    newer.sort_by_key(|t| t.after_seq);
}
