//! A foreman's or planner's `mork ctl say`, read from its command line and any shell line it wraps (fac-633): its
//! step leaves out the message when it reaches the card's other seat, or when the line doesn't show where it goes.

use serde_json::Value;

/// The words of a seat's step whose command says to its card's other seat, or says what its line doesn't place: that
/// it sent a message, never the message. None for any other command or seat.
pub fn words(seat: Option<&str>, line: &str) -> Option<String> {
    let (card, other) = other_seat(seat?)?;
    let all = wrapped(line, 4);
    let reached: Vec<Reach> =
        all.iter().filter_map(|cmd| said(cmd, line)).filter_map(|s| reach(&s, card, other)).collect();
    if reached.contains(&Reach::Other) {
        Some(format!("Sent a message to the {other}"))
    } else {
        reached.contains(&Reach::Unknown).then(|| "Sent a message".to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Other,
    Unknown,
}

// A seat's card and the other seat's name, from its key: `<card>/foreman`, its DM's `<card>/foreman/dm`, `<card>/planner`.
fn other_seat(key: &str) -> Option<(&str, &'static str)> {
    let parts: Vec<&str> = key.split('/').collect();
    match parts.as_slice() {
        [card, "foreman"] | [card, "foreman", "dm"] => Some((card, "planner")),
        [card, "planner"] => Some((card, "foreman")),
        _ => None,
    }
}

// A shell word unquoted; an expansion in it (`$`, a backtick) leaves its value open.
#[derive(Debug, Clone, Default)]
struct Word {
    text: String,
    open: bool,
}

// A command line's words, each command's apart: `;`, `&`, `|` and a line's end outside quotes close one.
fn commands(line: &str) -> Vec<Vec<Word>> {
    let mut out: Vec<Vec<Word>> = vec![Vec::new()];
    let mut word: Option<Word> = None;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), c) => word.get_or_insert_default().text.push(c),
            (_, '\\') => {
                if let Some(c) = chars.next().filter(|c| *c != '\n') {
                    word.get_or_insert_default().text.push(c);
                }
            }
            (_, '$' | '`') => {
                let w = word.get_or_insert_default();
                w.text.push(c);
                w.open = true;
            }
            (Some(_), c) => word.get_or_insert_default().text.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                word.get_or_insert_default();
            }
            (None, ';' | '&' | '|' | '\n') => {
                out.last_mut().into_iter().for_each(|cmd| cmd.extend(word.take()));
                out.push(Vec::new());
            }
            (None, c) if c.is_whitespace() => out.last_mut().into_iter().for_each(|cmd| cmd.extend(word.take())),
            (None, c) => word.get_or_insert_default().text.push(c),
        }
    }
    out.last_mut().into_iter().for_each(|cmd| cmd.extend(word.take()));
    out
}

// A line's commands, and those of each word that holds a command line a shell runs, as `bash -c '…'`, `eval '…'` or a
// variable's quoted value do: a word is shorter than its line, and `depth` bounds the nesting read.
fn wrapped(line: &str, depth: u8) -> Vec<Vec<Word>> {
    let mut out = commands(line);
    if depth > 0 {
        let held = |w: &&Word| w.text.contains("ctl") && w.text.contains(char::is_whitespace);
        let inner: Vec<String> = out.iter().flatten().filter(held).map(|w| w.text.clone()).collect();
        out.extend(inner.iter().flat_map(|text| wrapped(text, depth - 1)));
    }
    out
}

// What a `say` names: its place, its `to`, and its text's first word, each as written.
#[derive(Debug, Default)]
struct Said {
    place: Option<Word>,
    to: Option<Word>,
    lead: Option<Word>,
}

// A command's `say`, by its words (`ctl say <place> [--to <name>] <text>`, any field named) or its JSON; a JSON its
// line doesn't show, as `"$J"`, places nothing when the line holds a say.
fn said(cmd: &[Word], line: &str) -> Option<Said> {
    let at = cmd.iter().position(|w| w.text == "ctl")?;
    let mut rest = cmd.get(at + 1..).unwrap_or_default();
    if rest.first().is_some_and(|w| w.text == "--as") {
        rest = rest.get(2..).unwrap_or_default();
    }
    let (head, rest) = rest.split_first()?;
    match head.text.as_str() {
        "say" => Some(by_words(rest)),
        "--json" => by_json(rest.first()?, line),
        _ => None,
    }
}

fn by_json(json: &Word, line: &str) -> Option<Said> {
    if json.open {
        return line.contains("say").then(Said::default);
    }
    let v: Value = serde_json::from_str(&json.text).ok()?;
    let say = v.get("say")?;
    let field = |name: &str| say.get(name).and_then(Value::as_str).map(|s| Word { text: s.to_owned(), open: false });
    Some(Said { place: field("place"), to: field("to"), lead: field("text") })
}

fn by_words(rest: &[Word]) -> Said {
    let mut said = Said::default();
    let mut positional = Vec::new();
    let mut words = rest.iter();
    while let Some(w) = words.next() {
        let Some(flag) = w.text.strip_prefix("--") else {
            positional.push(w.clone());
            continue;
        };
        let value = words.next().cloned();
        match flag {
            "place" => said.place = value,
            "to" => said.to = value,
            "text" => said.lead = value,
            _ => {}
        }
    }
    let mut positional = positional.into_iter();
    if said.place.is_none() {
        said.place = positional.next();
    }
    if said.lead.is_none() {
        said.lead = positional.next();
    }
    said
}

// Whether a `say` reaches the other seat as the router places it: by its name, in any case, from the card or not, or
// unnamed in its room or the foreman's DM; what an expansion leaves open where it could is unknown.
fn reach(s: &Said, card: &str, other: &str) -> Option<Reach> {
    let names = |n: &str| {
        let n = n.trim().trim_start_matches('@').to_lowercase();
        let n = n.strip_prefix(card).and_then(|r| r.strip_prefix('/')).unwrap_or(&n);
        n == other || (other == "foreman" && n == "foreman/dm")
    };
    if let Some(to) = &s.to {
        return if to.open { Some(Reach::Unknown) } else { names(&to.text).then_some(Reach::Other) };
    }
    let Some(place) = s.place.as_ref().filter(|p| !p.open) else { return Some(Reach::Unknown) };
    let place = place.text.trim().to_lowercase();
    let spec = format!("{card}/spec");
    let theirs =
        if other == "planner" { place == spec } else { place == card || place == format!("{card}/foreman/dm") };
    if theirs {
        return Some(Reach::Other);
    }
    let on_card = place == card || place.strip_prefix(card).is_some_and(|suffix| suffix.starts_with('/'));
    let lead = s.lead.as_ref()?;
    let first = lead.text.split_whitespace().next().unwrap_or_default();
    if first.starts_with('@') && names(first) {
        Some(Reach::Other)
    } else {
        let open_name = !first.starts_with('@') || first.contains(['$', '`']);
        (on_card && lead.open && open_name).then_some(Reach::Unknown)
    }
}
