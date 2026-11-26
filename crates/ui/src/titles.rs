//! Card ids in the conversation's words drawn with their titles, `fac-43 (its title)` (fac-33,
//! fac-55): agents keep writing bare ids, and the stored and copied words keep them bare.

use std::collections::BTreeMap;

use mork_types::view::Screen;

use crate::markdown::{Block, Run};
use crate::text::clean;

/// Every card the boards show: its title, by its id.
pub type Titles = BTreeMap<String, String>;

/// The boards' titles; a card with none is left out.
pub fn of(screen: &Screen) -> Titles {
    let rows = screen.boards.iter().flat_map(crate::board::every_row);
    rows.map(|r| (r.id.to_string(), clean(r.title.trim()))).filter(|(_, t)| !t.is_empty()).collect()
}

/// The text with each known card id it names as a word followed by its title; an unknown id,
/// and one inside a longer name (`branch-fac-33`, `fac-33/foreman`), stays as written.
pub fn named(text: &str, titles: &Titles) -> String {
    if titles.is_empty() {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let Some(end) = id_at(&chars, i) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let id: String = chars[i..end].iter().collect();
        out.push_str(&id);
        if let Some(title) = titles.get(&id).or_else(|| titles.get(&id.to_ascii_lowercase())) {
            out.push_str(&format!(" ({title})"));
        }
        i = end;
    }
    out
}

/// A text's blocks with their words named, code left as written.
pub fn blocks(blocks: Vec<Block>, titles: &Titles) -> Vec<Block> {
    let runs = |rs: Vec<Run>| -> Vec<Run> {
        rs.into_iter().map(|r| if r.code || r.link { r } else { Run { text: named(&r.text, titles), ..r } }).collect()
    };
    let named_block = |b: Block| match b {
        Block::Line(rs) => Block::Line(runs(rs)),
        Block::Heading(rs) => Block::Heading(runs(rs)),
        Block::Item { depth, marker, runs: rs } => Block::Item { depth, marker, runs: runs(rs) },
        Block::Table { head, rows } => Block::Table {
            head: head.into_iter().map(runs).collect(),
            rows: rows.into_iter().map(|r| r.into_iter().map(runs).collect()).collect(),
        },
        other @ (Block::Gap | Block::Code(_)) => other,
    };
    blocks.into_iter().map(named_block).collect()
}

// Standalone ids avoid inserting titles inside branch names, paths and agent addresses.
fn id_at(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && joins(chars[i - 1]) {
        return None;
    }
    let letters = chars[i..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
    if letters == 0 || chars.get(i + letters) != Some(&'-') {
        return None;
    }
    let start = i + letters + 1;
    let digits = chars[start..].iter().take_while(|c| c.is_ascii_digit()).count();
    let end = start + digits;
    let free = match chars.get(end) {
        None => true,
        // A sentence's full stop ends the id; a file's extension doesn't.
        Some('.') => !chars.get(end + 1).is_some_and(|c| c.is_alphanumeric()),
        Some(c) => !joins(*c),
    };
    (digits > 0 && free).then_some(end)
}

fn joins(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.')
}
