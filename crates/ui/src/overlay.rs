//! `/help`, `/usage`, an agent's picker and its latest turn (fac-96) in the place's area, until Esc
//! (5.10.3).

use factory_types::catalog::{CATALOG, FieldType};
use iced::widget::{Column, Row, column, row};
use iced::{Element, Length};
use serde_json::Value;

use crate::app::{App, Message, Overlay};
use crate::fonts;
use crate::spans::Tone;
use crate::widgets::{self, bold, txt, wrapped};

const KEYS: [(&str, &str); 12] = [
    ("Enter", "send the line; on an empty one, open the room picked"),
    ("Shift+Enter", "a new line"),
    ("Tab", "unfold every message of the place, or fold them all"),
    ("Esc", "close this, or go back to main"),
    ("Ctrl+↑", "pick the room above"),
    ("Ctrl+↓", "pick the room below"),
    ("Ctrl+←", "fold the card picked; on main, the tab to its left"),
    ("Ctrl+→", "unfold the card picked; on main, the tab to its right"),
    ("Alt+1", "press the first button above the input"),
    ("Alt+9", "press the ninth"),
    ("↑", "scroll up; on the Board, the row above"),
    ("↓", "scroll down; on the Board, the row below"),
];

const FIELD_CHARS: f32 = 16.0;

// These send nothing, but `/usage`'s read.
const OWN: [(&str, &str); 3] = [
    ("/open <card>", "open a card's room, done or closed included"),
    ("/help", "this"),
    ("/usage", "every limit and context in full"),
];

pub fn view<'a>(app: &'a App, o: &'a Overlay) -> Element<'a, Message> {
    let (title, body) = match o {
        Overlay::Help => ("/help".to_owned(), help()),
        Overlay::Tunnels => ("web tunnels".to_owned(), crate::tunnels::view(app)),
        Overlay::Usage { answer: None } => {
            ("/usage".to_owned(), column![txt("asking the engine for every limit and context…", Tone::Dim)].into())
        }
        Overlay::Usage { answer: Some(v) } => ("/usage".to_owned(), usage(v)),
        Overlay::Pick { agent, draft } => (format!("pick  {agent}"), crate::picker::view(app, agent, draft)),
        Overlay::Turn { agent } => (format!("latest turn  {agent}"), crate::turn::view(app, agent)),
    };
    column![row![bold(title, Tone::Bright), txt("   Esc closes", Tone::Dim)], widgets::scroll(body)].into()
}

fn pair<'a>(key: String, words: String, tone: Tone) -> Row<'a, Message> {
    row![txt(key, tone).width(Length::Fixed(fonts::ADVANCE * 28.0)), txt(words, Tone::Text)]
}

fn help<'a>() -> Element<'a, Message> {
    let mut col = Column::new().push(txt("keys", Tone::Dim));
    for (k, w) in KEYS {
        col = col.push(pair(k.into(), w.into(), Tone::Bright));
    }
    col = col.push(txt("the window's commands", Tone::Dim));
    for (k, w) in OWN {
        col = col.push(pair(k.into(), w.into(), Tone::Bright));
    }
    col = col.push(txt("commands (1.13): the place and card are filled in from where they're typed", Tone::Dim));
    for c in CATALOG.iter().filter(|c| !c.faults_only) {
        let fields: Vec<String> = c
            .fields
            .iter()
            .map(|f| {
                let list = if f.ty == FieldType::StrList { "…" } else { "" };
                if f.optional && f.ty == FieldType::Bool {
                    format!("[--{}]", f.name)
                } else if f.optional {
                    format!("[--{} <{}{list}>]", f.name, f.name)
                } else {
                    format!("<{}{list}>", f.name)
                }
            })
            .collect();
        col = col.push(pair(format!("/{}", c.name), fields.join(" "), Tone::Green));
    }
    col.into()
}

fn usage<'a>(v: &Value) -> Element<'a, Message> {
    let mut col = Column::new();
    if let Some(lines) = v.get("by_pool").and_then(Value::as_array) {
        for line in lines.iter().filter_map(Value::as_str) {
            col = col.push(wrapped(line.to_owned(), Tone::Text));
        }
    }
    match v.as_object() {
        Some(obj) => {
            for (name, value) in obj.iter().filter(|(name, _)| *name != "by_pool") {
                col = col.push(txt(name.clone(), Tone::Dim));
                col = col.push(items(value));
            }
        }
        None => col = col.push(items(v)),
    }
    col.into()
}

fn items<'a>(v: &Value) -> Element<'a, Message> {
    let mut col = Column::new();
    match v {
        Value::Array(list) if list.is_empty() => col = col.push(txt("none", Tone::Dim)),
        Value::Array(list) => {
            for item in list {
                col = col.push(fields(item));
            }
        }
        other => col = col.push(fields(other)),
    }
    col.into()
}

// A field's value wraps to the place's width, so a long one shows whole (5.10.3).
fn fields<'a>(v: &Value) -> Element<'a, Message> {
    let field = |k: &str, val: &Value| -> Element<'a, Message> {
        let name = txt(k.to_owned(), Tone::Dim).width(Length::Fixed(fonts::ADVANCE * FIELD_CHARS));
        row![name, wrapped(scalar(val), Tone::Text).width(Length::Fill)].into()
    };
    let lines = match v.as_object() {
        Some(obj) => Column::with_children(obj.iter().map(|(k, val)| field(k, val))),
        None => Column::new().push(wrapped(scalar(v), Tone::Text)),
    };
    lines.padding(iced::Padding { bottom: fonts::LINE / 2.0, ..iced::Padding::ZERO }).into()
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => crate::text::clean(s),
        Value::Null => "?".into(),
        other => other.to_string(),
    }
}
