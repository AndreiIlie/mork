use std::ops::Range;
use toml_edit::{Document, InlineTable, Item, Key, Table, Value};

pub(super) type Result<T> = std::result::Result<T, String>;
const SHAPE: &str = "Could not locate the setting in mork.toml.";

fn line_start(s: &str, at: usize) -> usize {
    s[..at].rfind('\n').map_or(0, |i| i + 1)
}
fn after_line(s: &str, at: usize) -> usize {
    s[at..].find('\n').map_or(s.len(), |i| at + i + 1)
}

pub(super) fn doc(s: &str) -> Result<Document<&str>> {
    Document::parse(s).map_err(|_| SHAPE.into())
}

pub(super) fn item<'a>(root: &'a Item, path: &[&str]) -> Option<&'a Item> {
    path.iter().try_fold(root, |v, k| v.get(*k))
}

fn dotted(path: &[&str]) -> String {
    path.iter().map(|k| Key::new(*k).to_string()).collect::<Vec<_>>().join(".")
}

fn insert_line(s: &mut String, at: usize, line: &str) {
    let newline = if s.contains("\r\n") { "\r\n" } else { "\n" };
    let prefix = if at > 0 && !s[..at].ends_with('\n') { newline } else { "" };
    let suffix = if at < s.len() || s.ends_with('\n') { newline } else { "" };
    s.insert_str(at, &format!("{prefix}{line}{suffix}"));
}

fn remove_statement(s: &mut String, range: Range<usize>) {
    let end = after_line(s, range.end);
    let suffix = &s[range.end..end];
    let keep = suffix.find('#').map(|i| range.end + i).unwrap_or(end);
    let notes = removed_comments(&s[range.clone()]);
    s.replace_range(line_start(s, range.start)..keep, &notes);
}

fn removed_comments(raw: &str) -> String {
    let mut notes = String::new();
    if let Ok(v) = raw.parse::<Value>() {
        super::arrays::comments(&v, &mut notes);
    }
    notes
}

fn skip_trivia(s: &str, mut at: usize) -> usize {
    while let Some(byte) = s.as_bytes().get(at) {
        match byte {
            b'#' => at = after_line(s, at),
            ch if ch.is_ascii_whitespace() => at += 1,
            _ => break,
        }
    }
    at
}

fn remove_inline(s: &mut String, table: &InlineTable, target: Range<usize>) -> Result<()> {
    let begin = table.span().ok_or(SHAPE)?.start + 1;
    let mut spans: Vec<_> = table.get_values().into_iter().filter_map(|(_, v)| v.span()).collect();
    spans.sort_by_key(|r| r.start);
    let i = spans.iter().position(|r| *r == target).ok_or(SHAPE)?;
    let previous = i.checked_sub(1).map(|n| skip_trivia(s, spans[n].end));
    let start = skip_trivia(s, previous.map_or(begin, |comma| comma + 1));
    let after = skip_trivia(s, target.end);
    let notes = removed_comments(&s[target.clone()]);
    if s.as_bytes().get(after) == Some(&b',') {
        s.remove(after);
        s.replace_range(start..target.end, &notes);
    } else {
        s.replace_range(start..target.end, &notes);
        if let Some(comma) = previous {
            s.remove(comma);
        }
    }
    Ok(())
}

enum Parent<'a> {
    Table(&'a Table),
    Inline(&'a InlineTable),
}

// Absolute source spans keep dotted keys in file order, even when the parser groups them under the same table.
fn update(s: &str, path: &[&str], value: Option<Value>) -> Result<String> {
    let parsed = doc(s)?;
    let mut node = parsed.as_item();
    let mut owner = Parent::Table(parsed.as_table());
    let mut relative = path;
    let mut result = s.to_owned();
    for (idx, key) in path.iter().enumerate() {
        if let Some(t) = node.as_inline_table().filter(|t| !t.is_dotted()) {
            owner = Parent::Inline(t);
            relative = &path[idx..];
        } else if let Some(t) = node.as_table().filter(|t| !t.is_implicit()) {
            owner = Parent::Table(t);
            relative = &path[idx..];
        }
        let Some(next) = node.get(*key) else {
            if let Some(v) = value {
                add(&mut result, owner, relative, &v)?;
            }
            return Ok(result);
        };
        node = next;
    }
    let range = node.span().ok_or(SHAPE)?;
    if let Some(v) = value {
        if !node.is_value() {
            return Err(SHAPE.into());
        }
        result.replace_range(range.clone(), &super::arrays::formatted(&s[range], &v));
    } else if node.as_table_like().is_some_and(|t| !t.is_empty()) {
        return Err(SHAPE.into());
    } else {
        match owner {
            Parent::Inline(t) => remove_inline(&mut result, t, range)?,
            Parent::Table(_) => remove_statement(&mut result, range),
        }
    }
    Ok(result)
}

fn add(s: &mut String, parent: Parent<'_>, path: &[&str], value: &Value) -> Result<()> {
    let line = format!("{} = {value}", dotted(path));
    match parent {
        Parent::Table(t) => {
            let last = t.get_values().iter().filter_map(|(_, v)| v.span()).map(|r| r.end).max();
            let at =
                last.or_else(|| t.span().filter(|r| !r.is_empty()).map(|r| r.end)).map_or(0, |end| after_line(s, end));
            insert_line(s, at, &line);
        }
        Parent::Inline(t) => {
            let last = t.get_values().iter().filter_map(|(_, v)| v.span()).map(|r| r.end).max();
            let (at, sep) = last.map_or((t.span().ok_or(SHAPE)?.start + 1, ""), |end| (end, ", "));
            s.insert_str(at, &format!("{sep}{line}"));
        }
    }
    Ok(())
}

pub(super) fn set(s: &mut String, path: &[&str], value: Option<Value>) -> Result<()> {
    *s = update(s, path, value)?;
    Ok(())
}

pub(super) fn prune(s: &mut String, path: &[&str]) -> Result<()> {
    let parsed = doc(s.as_str())?;
    let empty = item(parsed.as_item(), path).is_some_and(|v| v.as_table_like().is_some_and(|t| t.is_empty()));
    if empty {
        set(s, path, None)?;
    }
    Ok(())
}

pub(super) fn remove_pick(s: &mut String, table: &Table) -> Result<()> {
    let mut statements: Vec<_> = table.iter().filter_map(|(_, v)| v.span()).collect();
    statements.push(table.span().ok_or(SHAPE)?);
    statements.sort_by_key(|r| std::cmp::Reverse(r.start));
    for range in statements {
        remove_statement(s, range);
    }
    Ok(())
}
