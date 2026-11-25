//! The comment rules (fac-2, fac-195): no run over 2 lines, doc comments only on items marked `pub`
//! (`pub(crate)` and `pub(super)` count), and no unfinished-work marker without a card id.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::{visibility, walk};

/// The longest run of comment lines allowed, documentation included.
pub const CAP: usize = 2;
/// What marks a generated file, rustfmt's marker.
pub const GENERATED: &str = "@generated";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Outer,
    Inner,
}

impl Kind {
    fn of(line_comment: &str) -> Self {
        if line_comment.starts_with("///") && !line_comment.starts_with("////") {
            Self::Outer
        } else if line_comment.starts_with("//!") {
            Self::Inner
        } else {
            Self::Plain
        }
    }

    // `/**/` and `/***` are plain, as rustc reads them.
    fn of_block(block: &str) -> Self {
        if block.starts_with("/**") && !block.starts_with("/***") && !block.starts_with("/**/") {
            Self::Outer
        } else if block.starts_with("/*!") {
            Self::Inner
        } else {
            Self::Plain
        }
    }
}

// A `//` line, or a whole block comment over `lines` lines.
#[derive(Debug, Clone, Copy)]
struct Comment<'a> {
    line: usize,
    lines: usize,
    kind: Kind,
    block: bool,
    starts_line: bool,
    text: &'a str,
}

impl Comment<'_> {
    fn marker(&self) -> &'static str {
        match (self.block, self.kind) {
            (false, Kind::Plain) => "//",
            (false, Kind::Outer) => "///",
            (false, Kind::Inner) => "//!",
            (true, Kind::Plain) => "/*",
            (true, Kind::Outer) => "/**",
            (true, Kind::Inner) => "/*!",
        }
    }
}

/// Every problem found under `root`, skipping `target/` and the files a comment in their first
/// 5 lines marks generated.
pub fn check(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut parsed = Vec::new();
    let mut texts = Vec::new();
    for file in walk::rust_files(root) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let rel = walk::rel(root, &file);
        match syn::parse_file(&text) {
            Ok(syntax) => parsed.push((PathBuf::from(&rel), syntax)),
            Err(e) => problems.push(format!("{rel}:{}: not Rust that `syn` parses: {e}", e.span().start().line)),
        }
        texts.push((PathBuf::from(&rel), rel, text));
    }
    let modules = visibility::module_files(&parsed);
    for (path, rel, text) in &texts {
        let Some((_, syntax)) = parsed.iter().find(|(p, _)| p == path) else { continue };
        let public = visibility::public_doc_lines(syntax, modules.get(path).copied().unwrap_or(false));
        problems.extend(check_text(rel, text, &public));
    }
    problems
}

/// The problems of one file's text, each as `<rel>:<line>: …`; `public` holds the lines of the doc
/// comments on public items. None for a generated file.
pub fn check_text(rel: &str, text: &str, public: &HashSet<usize>) -> Vec<String> {
    let all = comments(text);
    if all.iter().any(|c| c.line <= 5 && c.text.contains(GENERATED)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for run in runs(&all) {
        let first = run[0];
        let lines: usize = run.iter().map(|c| c.lines).sum();
        if lines > CAP {
            out.push(format!("{rel}:{}: a `{}` comment of {lines} lines, over {CAP}", first.line, first.marker()));
        }
        if first.kind != Kind::Plain {
            out.extend(doc_run(rel, &run, public));
        }
    }
    for c in &all {
        for (i, line) in c.text.lines().enumerate() {
            if let Some(m) = bare_marker(line) {
                out.push(format!("{rel}:{}: {m} without a card id", c.line + i));
            }
        }
    }
    out
}

/// Whether a line holds a card id, `[a-z]+-[0-9]+`.
pub fn has_card_id(line: &str) -> bool {
    let b = line.as_bytes();
    b.iter().enumerate().any(|(i, &c)| {
        c == b'-' && i > 0 && b[i - 1].is_ascii_lowercase() && b.get(i + 1).is_some_and(u8::is_ascii_digit)
    })
}

fn bare_marker(text: &str) -> Option<String> {
    let [todo, fixme, _] = crate::caps::markers();
    [todo, fixme].into_iter().find(|m| crate::caps::contains_word(text, m)).filter(|_| !has_card_id(text))
}

fn runs<'a>(all: &[Comment<'a>]) -> Vec<Vec<Comment<'a>>> {
    let mut out: Vec<Vec<Comment<'a>>> = Vec::new();
    // A doc comment after code still documents the next item, so it starts a run; a plain one doesn't.
    for c in all.iter().filter(|c| c.block || c.starts_line || c.kind != Kind::Plain) {
        match out.last_mut() {
            Some(run)
                if !c.block && run.last().is_some_and(|p| !p.block && p.kind == c.kind && p.line + 1 == c.line) =>
            {
                run.push(*c);
            }
            _ => out.push(vec![*c]),
        }
    }
    out
}

fn doc_run(rel: &str, run: &[Comment<'_>], public: &HashSet<usize>) -> Vec<String> {
    let mut out = Vec::new();
    if run.iter().any(|c| !public.contains(&c.line)) {
        let m = run[0].marker();
        out.push(format!("{rel}:{}: a `{m}` comment on an item not marked `pub`", run[0].line));
    }
    for c in run.iter().filter(|c| !c.block) {
        let body = c.text[c.marker().len()..].trim();
        if let Some(name) = heading(body).filter(|h| *h == "Arguments" || *h == "Returns") {
            out.push(format!("{rel}:{}: `# {name}` in a doc comment", c.line));
        }
    }
    out
}

// An ATX heading's name, its closing `#`s and a trailing `:` dropped; one with no space after its `#`s
// counts too, so a near miss isn't let through.
fn heading(body: &str) -> Option<&str> {
    let n = body.chars().take_while(|c| *c == '#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let rest = body[n..].trim();
    let closed = rest.trim_end_matches('#');
    let name = if closed.is_empty() || closed.ends_with(char::is_whitespace) { closed.trim_end() } else { rest };
    Some(name.trim_end_matches(':').trim_end())
}

struct Lexer<'a> {
    text: &'a str,
    at: usize,
    line: usize,
    line_start: usize,
    out: Vec<Comment<'a>>,
}

// Literals are lexed through, so a `//` inside a string is never taken for a comment.
fn comments(text: &str) -> Vec<Comment<'_>> {
    let mut l = Lexer { text, at: 0, line: 1, line_start: 0, out: Vec::new() };
    let b = text.as_bytes();
    while let Some(&c) = b.get(l.at) {
        match (c, b.get(l.at + 1)) {
            (b'\n', _) => l.newline(),
            (b'/', Some(b'/')) => l.line_comment(),
            (b'/', Some(b'*')) => l.block_comment(),
            (b'"', _) => l.string(),
            (b'\'', _) => l.char_or_lifetime(),
            _ => l.at += 1,
        }
    }
    l.out
}

impl<'a> Lexer<'a> {
    fn newline(&mut self) {
        self.at += 1;
        self.line += 1;
        self.line_start = self.at;
    }

    fn starts_line(&self, from: usize) -> bool {
        self.text[self.line_start..from].trim().is_empty()
    }

    fn line_comment(&mut self) {
        let end = self.text[self.at..].find('\n').map_or(self.text.len(), |n| self.at + n);
        let text = &self.text[self.at..end];
        let starts_line = self.starts_line(self.at);
        self.out.push(Comment { line: self.line, lines: 1, kind: Kind::of(text), block: false, starts_line, text });
        self.at = end;
    }

    // Block comments nest, as Rust's do.
    fn block_comment(&mut self) {
        let b = self.text.as_bytes();
        let (from, line) = (self.at, self.line);
        let starts_line = self.starts_line(from);
        let mut depth = 0usize;
        while let Some(&c) = b.get(self.at) {
            match (c, b.get(self.at + 1)) {
                (b'/', Some(b'*')) => (depth, self.at) = (depth + 1, self.at + 2),
                (b'*', Some(b'/')) => (depth, self.at) = (depth - 1, self.at + 2),
                (b'\n', _) => self.newline(),
                _ => self.at += 1,
            }
            if depth == 0 {
                break;
            }
        }
        let text = &self.text[from..self.at];
        let comment =
            Comment { line, lines: self.line - line + 1, kind: Kind::of_block(text), block: true, starts_line, text };
        self.out.push(comment);
    }

    // Raw when an `r` and its hashes stand before the quote; then `\` escapes nothing.
    fn string(&mut self) {
        let b = self.text.as_bytes();
        let hashes = b[..self.at].iter().rev().take_while(|&&c| c == b'#').count();
        let r = self.at.checked_sub(hashes + 1);
        let raw = r.is_some_and(|r| b[r] == b'r' && !ident_before(b, r));
        self.at += 1;
        while let Some(&c) = b.get(self.at) {
            match c {
                b'\n' => self.newline(),
                b'\\' if !raw => {
                    if b.get(self.at + 1) == Some(&b'\n') {
                        self.at += 1;
                        self.newline();
                    } else {
                        self.at += 2;
                    }
                }
                b'"' if !raw || b[self.at + 1..].iter().take(hashes).filter(|&&h| h == b'#').count() == hashes => {
                    self.at += 1 + if raw { hashes } else { 0 };
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    // `'a'` and `'\u{1F600}'` are chars, `'a` a lifetime: only a char closes its quote.
    fn char_or_lifetime(&mut self) {
        let rest = &self.text[self.at + 1..];
        let mut chars = rest.chars();
        match chars.next() {
            Some('\\') => {
                let skip = 1 + chars.next().map_or(0, char::len_utf8);
                self.at += 1 + skip + rest[skip..].find('\'').map_or(rest.len() - skip, |n| n + 1);
            }
            Some(c) if c != '\n' && chars.next() == Some('\'') => self.at += 2 + c.len_utf8(),
            _ => self.at += 1,
        }
    }
}

// An `r` ending an identifier isn't a raw string's; `br"…"` and `cr"…"` still are.
fn ident_before(b: &[u8], at: usize) -> bool {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    match at.checked_sub(1).map(|p| (p, b[p])) {
        Some((p, b'b' | b'c')) => p.checked_sub(1).is_some_and(|q| ident(b[q])),
        Some((_, c)) => ident(c),
        None => false,
    }
}
