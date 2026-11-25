//! The conversation's markdown (fac-39): each line break is kept as written, and anything not
//! recognised stays as written. The widgets are `prose`'s.

/// A run of words and their marks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: bool,
}

impl Run {
    fn marked(&self) -> bool {
        self.bold || self.italic || self.code || self.link
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A line of words, as written.
    Line(Vec<Run>),
    /// One or more empty lines between blocks.
    Gap,
    Heading(Vec<Run>),
    /// A list item: how deep it is, its marker (`•`, or its number as written), its words.
    Item {
        depth: usize,
        marker: String,
        runs: Vec<Run>,
    },
    /// A fenced block's lines, as written.
    Code(Vec<String>),
    /// A table: its header's cells, then each row's, as many as the header's.
    Table {
        head: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
}

/// The blocks of a text, with no gap first or last.
pub fn parse(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if fence(line) {
            let end = lines[i..].iter().position(|l| fence(l)).map_or(lines.len(), |n| i + n);
            out.push(Block::Code(lines[i..end].iter().map(|l| (*l).to_owned()).collect()));
            i = (end + 1).min(lines.len());
        } else if line.trim().is_empty() {
            if !matches!(out.last(), None | Some(Block::Gap)) {
                out.push(Block::Gap);
            }
        } else if let Some(sep) = lines.get(i).filter(|next| line.contains('|') && separator(next)) {
            let head = cells(line);
            let width = head.len().max(cells(sep).len());
            let rows: Vec<&str> =
                lines[i + 1..].iter().take_while(|l| l.contains('|') && !l.trim().is_empty()).copied().collect();
            i += 1 + rows.len();
            let fit = |mut c: Vec<Vec<Run>>| {
                c.resize(width, Vec::new());
                c
            };
            out.push(Block::Table { head: fit(head), rows: rows.into_iter().map(|r| fit(cells(r))).collect() });
        } else {
            out.push(block(line));
        }
    }
    if out.last() == Some(&Block::Gap) {
        out.pop();
    }
    out
}

fn block(line: &str) -> Block {
    let body = line.trim_start();
    let hashes = body.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && body[hashes..].starts_with(' ') {
        let text = body[hashes..].trim();
        let without_hashes = text.trim_end_matches('#');
        let text = if without_hashes.ends_with(char::is_whitespace) { without_hashes.trim_end() } else { text };
        return Block::Heading(inline(text));
    }
    let depth = (line.len() - body.len()) / 2;
    if let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|m| body.strip_prefix(m)) {
        return Block::Item { depth, marker: "•".into(), runs: inline(rest.trim_start()) };
    }
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    let after = &body[digits..];
    if (1..=9).contains(&digits) && (after.starts_with(". ") || after.starts_with(") ")) {
        return Block::Item { depth, marker: body[..=digits].to_owned(), runs: inline(after[2..].trim_start()) };
    }
    Block::Line(inline(line))
}

fn fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

// `|---|:--:|`: a table's second line.
fn separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('-')
        && t.contains(['|', ':'])
        && t.trim_matches('|').split('|').all(|c| {
            let c = c.trim();
            !c.is_empty() && c.trim_matches(':').chars().all(|ch| ch == '-')
        })
}

// A row's cells, split on the pipes outside code and not escaped, the outer ones dropped.
fn cells(line: &str) -> Vec<Vec<Run>> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').filter(|r| !r.ends_with('\\')).unwrap_or(t);
    let mut out = Vec::new();
    let (mut cell, mut code, mut escaped) = (String::new(), false, false);
    for c in t.chars() {
        match c {
            '|' if !code && !escaped => out.push(std::mem::take(&mut cell)),
            '`' if !escaped => {
                code = !code;
                cell.push(c);
            }
            _ => cell.push(c),
        }
        escaped = c == '\\' && !escaped;
    }
    out.push(cell);
    out.into_iter().map(|c| inline(c.trim())).collect()
}

/// A line's runs: its bold, italic and inline code marked, the marks themselves gone.
pub fn inline(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    spans(&chars, Run::default(), &mut out);
    out
}

/// A line's words with its marks gone: a folded preview's.
pub fn plain(text: &str) -> String {
    inline(text).into_iter().map(|r| r.text).collect()
}

/// Whether runs carry any mark, so a rich text must draw them.
pub fn marked(runs: &[Run]) -> bool {
    runs.iter().any(Run::marked)
}

fn spans(chars: &[char], marks: Run, out: &mut Vec<Run>) {
    let mut i = 0;
    let mut words = String::new();
    while i < chars.len() {
        let c = chars[i];
        if let Some(n) = crate::urls::length(&chars[i..]) {
            push(out, &marks, std::mem::take(&mut words));
            push(out, &Run { link: true, ..marks.clone() }, chars[i..i + n].iter().collect());
            i += n;
            continue;
        }
        if c == '\\' && chars.get(i + 1).is_some_and(char::is_ascii_punctuation) {
            words.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if let Some((inner, end, add)) = code(chars, i).or_else(|| emphasis(chars, i)) {
            push(out, &marks, std::mem::take(&mut words));
            let marked = Run { bold: marks.bold || add.bold, italic: marks.italic || add.italic, ..marks.clone() };
            if add.code {
                out.extend(crate::urls::runs(Run { text: inner.iter().collect(), code: true, ..marked }));
            } else {
                spans(inner, marked, out);
            }
            i = end;
            continue;
        }
        words.push(c);
        i += 1;
    }
    push(out, &marks, words);
}

fn push(out: &mut Vec<Run>, marks: &Run, text: String) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last)
            if !last.link
                && !marks.link
                && (last.bold, last.italic, last.code) == (marks.bold, marks.italic, marks.code) =>
        {
            last.text.push_str(&text);
        }
        _ => out.push(Run { text, ..marks.clone() }),
    }
}

type Found<'c> = (&'c [char], usize, Run);

// A code span at `i`: as many backticks close it as opened it.
fn code(chars: &[char], i: usize) -> Option<Found<'_>> {
    if chars[i] != '`' || (i > 0 && chars[i - 1] == '`') {
        return None;
    }
    let n = chars[i..].iter().take_while(|c| **c == '`').count();
    let from = i + n;
    let mut j = from;
    while j < chars.len() {
        let run = chars[j..].iter().take_while(|c| **c == '`').count();
        if run == n {
            let inner = &chars[from..j];
            let trimmed = if inner.len() > 2 && inner[0] == ' ' && inner[inner.len() - 1] == ' ' {
                &inner[1..inner.len() - 1]
            } else {
                inner
            };
            return Some((trimmed, j + n, Run { code: true, ..Run::default() }));
        }
        j += run.max(1);
    }
    None
}

// Bold, italic or both at `i`: a run of `*` or `_` that opens before a word and closes after
// one, `_` only between words, so `snake_case` and `a * b` stay as written.
fn emphasis(chars: &[char], i: usize) -> Option<Found<'_>> {
    let d = chars[i];
    if !matches!(d, '*' | '_') || (i > 0 && chars[i - 1] == d) {
        return None;
    }
    let n = chars[i..].iter().take_while(|c| **c == d).count();
    let from = i + n;
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    let opens = chars.get(from).is_some_and(|c| !c.is_whitespace()) && !(d == '_' && i > 0 && word(chars.get(i - 1)));
    if n > 3 || !opens {
        return None;
    }
    let mut j = from + 1;
    while j + n <= chars.len() {
        if let Some((_, end, _)) = code(chars, j) {
            j = end;
            continue;
        }
        let closes = chars[j..j + n].iter().all(|c| *c == d)
            && chars.get(j + n) != Some(&d)
            && !chars[j - 1].is_whitespace()
            && !(d == '_' && word(chars.get(j + n)));
        if closes {
            let add = Run { bold: n >= 2, italic: n != 2, ..Run::default() };
            return Some((&chars[from..j], j + n, add));
        }
        j += 1;
    }
    None
}
