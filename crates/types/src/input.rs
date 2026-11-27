//! Reading a line typed in a place (5.10): a command, a place to go to, or a note saying what
//! is wrong. It changes nothing; the buttons go through it too.

use serde::{Deserialize, Serialize};

use crate::Command;
use crate::catalog::{self, CommandSpec};
use crate::command::Delivery;

/// Where the line is typed: the place, its card, who plain text goes to (empty: whoever the
/// engine gives the place's unaddressed words to).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub place: String,
    pub card: Option<String>,
    pub to: String,
}

/// What a line means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// A command of 1.13, to send once.
    Send(Command),
    /// `/open <card>`: go there; nothing is sent.
    Open(String),
    /// `/help`: every command and key; nothing is sent.
    Help,
    /// `/usage`: 1.13's `usage`, a read, shown in the place's area.
    Usage(Command),
    /// What is wrong with it; nothing is sent.
    Note(String),
    /// An empty line.
    Nothing,
}

// Commands that always name their card: `/start` alone is malformed even in a room, whose own
// start is the engine's button (T5.11).
const NAMES_ITS_CARD: [&str; 1] = ["start"];

/// Reads a line typed in a place.
pub fn parse(line: &str, ctx: &Context) -> Parsed {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Parsed::Nothing;
    }
    match trimmed.strip_prefix('/') {
        Some(rest) => slash(rest, ctx),
        None => say(trimmed, &[], ctx),
    }
}

/// Reads a draft: its line and the images pasted into it (fac-31). A plain line carries them,
/// and so do images alone; a `/` line is read as `parse` reads it, without them.
pub fn draft(line: &str, images: &[String], ctx: &Context) -> Parsed {
    let trimmed = line.trim();
    if images.is_empty() || trimmed.starts_with('/') {
        return parse(line, ctx);
    }
    say(trimmed, images, ctx)
}

/// Applies the requested delivery to a draft. A command submitted with Send now stays in the editor.
pub fn submission(line: &str, images: &[String], ctx: &Context, now: bool) -> Result<Parsed, &'static str> {
    if now && line.trim_start().starts_with('/') {
        return Err("✗ send now sends a message, not a command: nothing sent");
    }
    let mut parsed = draft(line, images, ctx);
    if now && let Parsed::Send(Command::Say { delivery, .. }) = &mut parsed {
        *delivery = Some(Delivery::Now);
    }
    Ok(parsed)
}

// The engine says whether a leading `@name` reaches anyone (3.4.5).
fn say(text: &str, images: &[String], ctx: &Context) -> Parsed {
    let (to, text) = match text.strip_prefix('@').map(|r| r.split_once(char::is_whitespace).unwrap_or((r, ""))) {
        Some(("", _)) => return Parsed::Note("✗ @ needs a name: @foreman".into()),
        Some((name, rest)) => (name.to_owned(), rest.trim()),
        None => (ctx.to.clone(), text),
    };
    if text.is_empty() && images.is_empty() {
        return Parsed::Note(format!("✗ nothing to say to {to}: nothing sent"));
    }
    let to = (!to.is_empty()).then_some(to);
    let images = (!images.is_empty()).then(|| images.to_vec());
    Parsed::Send(Command::Say {
        place: ctx.place.as_str().into(),
        to,
        text: text.to_owned(),
        images,
        kind: None,
        delivery: None,
        choice: None,
    })
}

fn slash(rest: &str, ctx: &Context) -> Parsed {
    let words = match shell_words(rest) {
        Ok(w) => w,
        Err(e) => return Parsed::Note(format!("✗ {e}: nothing sent")),
    };
    let Some((name, args)) = words.split_first() else {
        return Parsed::Note("✗ a command needs its name: /help lists them".into());
    };
    match name.as_str() {
        "help" => return Parsed::Help,
        "usage" if args.is_empty() => return Parsed::Usage(Command::Usage {}),
        "open" => {
            return match args {
                [card] => Parsed::Open(card.clone()),
                _ => Parsed::Note("✗ /open takes one card: /open ex-550".into()),
            };
        }
        _ => {}
    }
    let Some(spec) = catalog::find(name, false) else {
        return Parsed::Note(format!("✗ /{name} is no command: /help lists them"));
    };
    // A start's words go to the shared reader as typed: the card first, then those it carries (fac-563).
    let args = if NAMES_ITS_CARD.contains(&spec.name) { args.to_vec() } else { fill(spec, args, ctx) };
    match crate::words::command(spec, &args) {
        Ok(v) => read(&v),
        Err(e) => Parsed::Note(format!("✗ {e}: nothing sent")),
    }
}

fn read(v: &serde_json::Value) -> Parsed {
    match Command::parse(v, false) {
        Ok(Command::Usage {}) => Parsed::Usage(Command::Usage {}),
        Ok(c) => Parsed::Send(c),
        Err(e) => Parsed::Note(format!("✗ {}: nothing sent", e.message)),
    }
}

// A `/` line's words, as `mork ctl`'s shell splits them (5.10.2). A quote opens a stretch only at a
// word's start, so an apostrophe inside a word (`it's`) stays a letter.
fn shell_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.get_or_insert_with(String::new).push(c),
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    word.get_or_insert_with(String::new).push(next);
                }
            }
            (Some(_), c) => word.get_or_insert_with(String::new).push(c),
            (None, '"' | '\'') if word.is_none() => {
                quote = Some(c);
                word = Some(String::new());
            }
            (None, c) if c.is_whitespace() => words.extend(word.take()),
            (None, c) => word.get_or_insert_with(String::new).push(c),
        }
    }
    if quote.is_some() {
        return Err("a quote isn't closed".into());
    }
    words.extend(word);
    Ok(words)
}

fn fill(spec: &CommandSpec, args: &[String], ctx: &Context) -> Vec<String> {
    let (mut named, mut positional) = split(args);
    // `/fresh foreman` or `/pick foreman` in a room is its card's foreman; in main, main's own
    // (fac-25, fac-96).
    if (spec.name == "fresh" || spec.name == "pick")
        && let Some(w) = positional.first_mut().filter(|w| !w.contains('/'))
    {
        *w = format!("{}/{w}", ctx.card.as_deref().unwrap_or(&ctx.place));
    }
    let given = |named: &[(String, String)], f: &str| named.iter().any(|(n, _)| n == f);
    let required: Vec<_> = spec.fields.iter().filter(|f| !f.optional).collect();
    if let Some(first) = required.first().filter(|_| !NAMES_ITS_CARD.contains(&spec.name)) {
        let from_here = match first.name {
            "place" => Some(ctx.place.clone()),
            "card" => ctx.card.clone(),
            _ => None,
        };
        let named_here = positional.first().is_some_and(|w| names_a_place(w));
        if let Some(v) = from_here.filter(|_| !given(&named, first.name) && !named_here) {
            positional.insert(0, v);
        }
    }
    if spec.fields.iter().any(|f| f.optional && f.name == "card") && !given(&named, "card") {
        // A word for a required field, or the place the room filled in, is never taken again as
        // the card: only a command with no required field gives it its first word (`/stop w-30`).
        if required.is_empty() && positional.first().is_some_and(|w| names_a_place(w)) {
            named.push(("card".into(), positional.remove(0)));
        } else if let Some(card) = ctx.card.clone().filter(|_| !given(&named, "agent")) {
            named.push(("card".into(), card));
        }
    }
    let mut out = positional;
    for (n, v) in named {
        out.extend([format!("--{n}"), v]);
    }
    out
}

fn split(args: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let mut named = Vec::new();
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.strip_prefix("--"), it.clone().next()) {
            (Some(n), Some(v)) => {
                named.push((n.to_owned(), v.clone()));
                it.next();
            }
            _ => positional.push(a.clone()),
        }
    }
    (named, positional)
}

/// Whether a place is a card's challenge place, its foreman's and planner's exchange: the window opens it nowhere
/// (fac-633), `history` and `watch` read it.
pub fn shown_nowhere(place: &str) -> bool {
    place.split_once('/').is_some_and(|(card, rest)| rest == "challenge" && card != "main" && names_a_place(card))
}

/// Gives the operator the history command for a place that cannot be opened in either window.
pub fn open_refusal(place: &str) -> Option<String> {
    shown_nowhere(place).then(|| format!(
        "✗ {place} is the foreman's and planner's exchange, which the window shows nowhere: `mork ctl history {place}` reads it"
    ))
}

/// Whether a word names a place: `main`, a card's id (`ex-550`, `ex-12.1`, `pr-ex-12`), or a DM
/// (`ex-550/foreman/dm`).
pub fn names_a_place(word: &str) -> bool {
    if word == "main" {
        return true;
    }
    let card = word.split('/').next().unwrap_or(word);
    let Some((prefix, number)) = card.rsplit_once('-') else { return false };
    let number_ok =
        !number.is_empty() && number.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let prefix_ok =
        !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    number_ok && prefix_ok
}
