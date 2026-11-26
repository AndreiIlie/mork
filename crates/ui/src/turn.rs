//! An agent's latest turn over its place (fac-96): only the row's own job's, a child's never
//! shows in its parent's.

use iced::widget::{Column, container, responsive};
use iced::{Element, Length, Padding};
use mork_types::seen::{Detail, DetailKind, TurnView};

use crate::app::{App, Message};
use crate::fonts;
use crate::spans::Tone;
use crate::text::clean;
use crate::widgets::{self, clickable, cut, txt, wrapped};

/// The agent's latest turn, from the view of the place open.
pub fn view<'a>(app: &'a App, agent: &str) -> Element<'a, Message> {
    let row = app.place().and_then(|v| v.agents.iter().find(|a| a.agent.as_str() == agent));
    let Some(t) = row.and_then(|a| a.turn.as_ref()) else {
        return txt("no turn to show: none ran since the engine started, and none is kept", Tone::Dim).into();
    };
    let n = t.steps.len();
    let mut col = Column::new().push(txt(format!("job {} · {n} step{}", t.job, plural(n)), Tone::Dim));
    if t.cut {
        col = col.push(txt("earlier steps or texts were left out, to keep within bounds", Tone::Orange));
    }
    let all = || t.steps.iter().flat_map(|s| &s.details).chain(&t.after);
    // What was left out may have held some.
    if !all().any(|d| d.kind == DetailKind::Thinking) {
        let none = if t.cut { "no thinking in what is kept" } else { "no thinking: its stream gave none" };
        col = col.push(txt(none, Tone::Dim));
    }
    for s in &t.steps {
        col = col.push(wrapped(format!("· {}", clean(&s.words)), Tone::Text));
        col = details(app, col, t, &s.details);
    }
    if !t.after.is_empty() {
        col = col.push(txt("after its last step", Tone::Dim));
        col = details(app, col, t, &t.after);
    }
    col.into()
}

// A text's fold, by its job and its id there: a stream's ids repeat across turns, and its place
// moves as what is before it is left out.
fn key(t: &TurnView, d: &Detail) -> String {
    format!("{}:{}", t.job, d.id)
}

fn details<'a>(app: &App, mut col: Column<'a, Message>, t: &TurnView, list: &'a [Detail]) -> Column<'a, Message> {
    for d in list {
        let key = key(t, d);
        let open = app.details_open.contains(&key);
        let (kind, tone) = match d.kind {
            DetailKind::Text => ("text", Tone::Text),
            DetailKind::Thinking => ("thinking", Tone::Strong),
        };
        let who = if d.sub { "subagent's " } else { "" };
        let text = clean(&d.text);
        let head: Element<'a, Message> = if open {
            txt(format!("▾ {who}{kind}"), tone).into()
        } else {
            let head = format!("▸ {who}{kind}  {}", text.lines().next().unwrap_or(""));
            responsive(move |size| txt(cut(&head, widgets::chars_in(size.width)), tone).into())
                .height(Length::Fixed(fonts::LINE))
                .into()
        };
        let head = container(head).padding(Padding::ZERO.left(fonts::ADVANCE * 2.0));
        col = col.push(clickable(format!("detail:{key}"), head, Some(Message::Detail(key))));
        if open {
            let pad = Padding::ZERO.left(fonts::ADVANCE * 4.0);
            col = col.push(
                container(wrapped(text, if d.kind == DetailKind::Text { Tone::Text } else { Tone::Dim })).padding(pad),
            );
            if d.cut {
                col = col.push(container(txt("the text goes on past what is kept", Tone::Orange)).padding(pad));
            }
        }
    }
    col
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
