//! One sentence per line (fac-645): the manuals, roles and skills keep each prose sentence on a line of
//! its own, so two cards editing different sentences of one paragraph don't conflict.

use std::path::Path;

use pulldown_cmark::{Event, Options, Parser, TagEnd};

use crate::walk;

/// The folders whose Markdown is held to one sentence per line.
pub const FOLDERS: [&str; 3] = ["manuals", "roles", "skills"];

// Inline code and inline HTML stand for one word; a line break or a block's edge ends the run.
const OPAQUE: char = '\u{FFFC}';
const BREAK: char = '\n';
const CLOSERS: &[char] = &[')', ']', '"', '\'', '”', '’', '»'];
const QUOTES: &[char] = &['"', '\'', '”', '’', '»'];
const OPENERS: &[char] = &['(', '[', '"', '\'', '“', '‘', '«'];
const ABBREVIATIONS: [&str; 3] = ["vs", "cf", "approx"];

/// Where one prose sentence ends and the next begins on the same source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Boundary {
    /// The byte just past the first sentence's last character, its closing quote or bracket included.
    pub end: usize,
    /// The byte where the next sentence's first visible character starts.
    pub next: usize,
}

// One visible character of the prose and its byte in the source.
#[derive(Debug, Clone, Copy)]
struct Mark {
    ch: char,
    at: usize,
}

fn skipped(tag: TagEnd) -> bool {
    matches!(tag, TagEnd::Heading(_) | TagEnd::Table | TagEnd::CodeBlock | TagEnd::MetadataBlock(_) | TagEnd::HtmlBlock)
}

// Emphasis and links leave the run unbroken: only their words are seen.
fn inline(tag: TagEnd) -> bool {
    matches!(tag, TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link | TagEnd::Image)
}

// A text's characters at their own bytes, or all at its start when escapes or entities make it differ.
fn push_text(marks: &mut Vec<Mark>, source: &str, text: &str, range: std::ops::Range<usize>) {
    let exact = source.get(range.clone()) == Some(text);
    marks.extend(text.char_indices().map(|(i, ch)| Mark { ch, at: if exact { range.start + i } else { range.start } }));
}

// The prose's visible characters: paragraphs and list items, without headings, tables, code or metadata.
fn marks(source: &str) -> Vec<Mark> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_FOOTNOTES;
    let mut out = Vec::new();
    let mut skip = 0usize;
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        let at = range.start;
        match event {
            Event::Start(tag) if skipped(tag.to_end()) => skip += 1,
            Event::End(tag) if skipped(tag) => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Event::Start(tag) if inline(tag.to_end()) => {}
            Event::End(tag) if inline(tag) => {}
            Event::Text(text) => push_text(&mut out, source, &text, range),
            Event::Code(_) | Event::InlineHtml(_) | Event::FootnoteReference(_) => {
                out.push(Mark { ch: OPAQUE, at });
            }
            _ => out.push(Mark { ch: BREAK, at }),
        }
    }
    out
}

// The word before the terminator at `i`, its own inner dots included.
fn word_before(m: &[Mark], i: usize) -> String {
    let start = m[..i].iter().rposition(|c| !(c.ch.is_alphanumeric() || c.ch == '.')).map_or(0, |p| p + 1);
    m[start..i].iter().map(|c| c.ch).collect()
}

// A number after a colon, a sentence's end or a line's start counts items ("tests: 1. Modules …").
fn enumerator(m: &[Mark], i: usize, word: &str) -> bool {
    if word.is_empty() || !word.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let before = m[..i - word.chars().count()].iter().rev().find(|c| c.ch != ' ');
    before.is_none_or(|c| matches!(c.ch, ':' | '.' | BREAK))
}

// The first character from `k` on past any opening brackets and quotes.
fn opening(m: &[Mark], k: usize) -> Option<char> {
    m[k..].iter().map(|c| c.ch).find(|ch| !OPENERS.contains(ch))
}

// What may open a sentence: a capital, a digit, inline code, or a lowercase name.
fn opens(m: &[Mark], k: usize) -> bool {
    opening(m, k).is_some_and(|ch| ch.is_uppercase() || ch.is_ascii_digit() || ch == OPAQUE || ch.is_lowercase())
}

// A word before a `.` that doesn't close a sentence: an abbreviation, dotted (`U.S.`) or listed, a counted item,
// or `No.` before a number.
fn continues(m: &[Mark], start: usize, k: usize) -> bool {
    let word = word_before(m, start);
    let next = opening(m, k);
    let numbered = word.eq_ignore_ascii_case("no") && m[k].ch.is_ascii_digit();
    let dotted = word.contains('.')
        && word.split('.').all(|part| (1..=2).contains(&part.chars().count()) && part.chars().all(char::is_alphabetic));
    // `etc.` may also end its sentence: only a lowercase word after it carries the sentence on.
    let listed = word.eq_ignore_ascii_case("etc") && next.is_some_and(char::is_lowercase);
    numbered || dotted || listed || ABBREVIATIONS.contains(&word.to_lowercase().as_str()) || enumerator(m, start, &word)
}

// A sentence ends at the last of a run of `?` and `!`, or at one `.`; a run holding a `.` is an ellipsis. An ending
// inside a quotation may go on after it (“Is it ready?” he asks), so it ends none.
fn boundary(m: &[Mark], i: usize) -> Option<Boundary> {
    let terminal = |j: usize| m.get(j).is_some_and(|c| matches!(c.ch, '.' | '?' | '!'));
    if !terminal(i) || terminal(i + 1) {
        return None;
    }
    let start = m[..i].iter().rposition(|c| !matches!(c.ch, '.' | '?' | '!')).map_or(0, |p| p + 1);
    if start < i && m[start..=i].iter().any(|c| c.ch == '.') {
        return None;
    }
    let j = i + 1 + m[i + 1..].iter().take_while(|c| CLOSERS.contains(&c.ch)).count();
    let k = j + m[j..].iter().take_while(|c| matches!(c.ch, ' ' | '\t')).count();
    if k == j
        || m[i + 1..j].iter().any(|c| QUOTES.contains(&c.ch))
        || !opens(m, k)
        || (m[i].ch == '.' && continues(m, start, k))
    {
        return None;
    }
    let last = m[j - 1];
    Some(Boundary { end: last.at + last.ch.len_utf8(), next: m[k].at })
}

/// Every place in a Markdown text where a prose sentence ends and another begins on the same line.
pub fn boundaries(source: &str) -> Vec<Boundary> {
    let m = marks(source);
    let one_line = |b: &Boundary| source.as_bytes().get(b.end..b.next).is_some_and(|gap| !gap.contains(&b'\n'));
    (0..m.len()).filter_map(|i| boundary(&m, i)).filter(one_line).collect()
}

/// Every line of the manuals, roles and skills that holds the end of one sentence and the start of another.
pub fn check(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for folder in FOLDERS {
        for f in walk::files(&root.join(folder), "md") {
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let rel = walk::rel(root, &f);
            let line = |at: usize| text.as_bytes()[..at].iter().filter(|b| **b == b'\n').count() + 1;
            let mut lines: Vec<usize> = boundaries(&text).iter().map(|b| line(b.next)).collect();
            lines.dedup();
            out.extend(lines.into_iter().map(|n| format!("{rel}:{n}: more than one sentence on this line")));
        }
    }
    out
}
