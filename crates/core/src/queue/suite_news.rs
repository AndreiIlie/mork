//! A failed candidate's news (fac-164): each delivery's log line names the receiving card's copy
//! of its whole log. Its failing tests go unmarked, as no base tip's run compares them (fac-311).

use mork_types::CardId;
use mork_types::suites::Told;

use crate::post::Poster;

// A delivery of a failure's news: its log's line naming the receiving card's copy.
pub(crate) fn send(p: &mut Poster<'_>, from: &CardId, told: Told) {
    let path = p.card(from).and_then(|c| c.news?.logs.get(to(&told)).cloned());
    match path {
        Some(path) => deliver(p, with_text(told, |t| pointed(t, &path))),
        None => deliver(p, told),
    }
}

// Its news told: done with.
pub(crate) fn told(p: &mut Poster<'_>, id: &CardId) {
    let Some(mut c) = p.card(id).filter(|c| c.news.is_some()) else { return };
    c.news = None;
    p.put_card(c);
}

fn to(told: &Told) -> &CardId {
    match told {
        Told::Operator { card, .. } | Told::Foreman { card, .. } | Told::Fix { card, .. } => card,
    }
}

fn with_text(told: Told, f: impl Fn(&str) -> String) -> Told {
    match told {
        Told::Operator { card, text } => Told::Operator { card, text: f(&text) },
        Told::Foreman { card, headline, text } => Told::Foreman { card, headline, text: f(&text) },
        Told::Fix { card, mut fix } => {
            fix.text = f(&fix.text);
            Told::Fix { card, fix }
        }
    }
}

fn pointed(text: &str, path: &str) -> String {
    let line = |l: &str| if l.starts_with("The full log: ") { format!("The full log: {path}") } else { l.to_owned() };
    text.lines().map(line).collect::<Vec<_>>().join("\n")
}

fn deliver(p: &mut Poster<'_>, told: Told) {
    match told {
        Told::Operator { card, text } => crate::reviews::cards::to_operator(p, &card, &text),
        Told::Foreman { card, headline, text } => {
            crate::messages::requests::ask_fix_or_join(p, &card, &headline, &text);
        }
        Told::Fix { card, fix } => super::joins::hold_fix(p, &card, fix),
    }
}
