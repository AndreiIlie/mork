use super::notes::{self, Written};
use super::{BEGIN, BoardError, END};

/// A card's file, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFile {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Its `labels`, inline (`[a, b]`) or one per line.
    pub labels: Vec<String>,
    /// The whole description section, its notes included.
    pub description: String,
    pub acceptance: Vec<String>,
    pub crlf: bool,
    /// The dated notes on top of the description, newest first, and what is under them.
    pub notes: Vec<String>,
    pub body: String,
    /// Each acceptance criterion, and whether it is checked.
    pub criteria: Vec<(bool, String)>,
}

fn scalar(v: &str) -> String {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else if let Some(inner) = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        inner.replace("\\\"", "\"")
    } else {
        v.to_owned()
    }
}

pub(super) fn front(lines: &[&str]) -> Result<usize, BoardError> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Err(BoardError::Malformed("no front matter: the file doesn't start with `---`"));
    }
    lines
        .iter()
        .skip(1)
        .position(|l| l.trim_end() == "---")
        .map(|i| i + 1)
        .ok_or(BoardError::Malformed("the front matter isn't closed"))
}

fn field(lines: &[&str], end: usize, key: &str) -> Option<String> {
    lines[1..end].iter().find_map(|l| l.strip_prefix(&format!("{key}:")).map(scalar))
}

fn labels(lines: &[&str], end: usize) -> Vec<String> {
    let Some(at) = (1..end).find(|&i| lines[i].starts_with("labels:")) else { return Vec::new() };
    let value = lines[at]["labels:".len()..].trim();
    let items: Vec<String> = match value.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(list) => list.split(',').map(scalar).collect(),
        None => lines[at + 1..end]
            .iter()
            .map_while(|l| l.starts_with(' ').then(|| l.trim_start().strip_prefix("- ")).flatten())
            .map(scalar)
            .collect(),
    };
    items.into_iter().filter(|s| !s.is_empty()).collect()
}

pub fn parse(text: &str) -> Result<CardFile, BoardError> {
    read(text, &[])
}

/// Reads a card's file, its notes told apart by the notes Mork wrote.
pub fn read(text: &str, written: &[Written]) -> Result<CardFile, BoardError> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let end = front(&lines)?;
    let id = field(&lines, end, "id")
        .filter(|s| !s.is_empty())
        .ok_or(BoardError::Malformed("no `id` in the front matter"))?;
    if !valid_id(&id) {
        return Err(BoardError::Malformed("the id isn't `<prefix>-<n>`: a card's id names its folders"));
    }
    let title = field(&lines, end, "title").unwrap_or_default();
    let status = field(&lines, end, "status").unwrap_or_default();
    let labels = labels(&lines, end);
    let body = &lines[end + 1..];
    let section = match (body.iter().position(|l| l.trim() == BEGIN), body.iter().position(|l| l.trim() == END)) {
        (Some(b), Some(e)) if b < e => &body[b + 1..e],
        _ => &[],
    };
    let description = section.join("\n").trim().to_owned();
    let n = notes::split(section, written);
    let notes = n.notes;
    let under = section[n.end..].join("\n").trim().to_owned();
    let criteria: Vec<(bool, String)> = match (
        body.iter().position(|l| l.trim() == "<!-- AC:BEGIN -->"),
        body.iter().position(|l| l.trim() == "<!-- AC:END -->"),
    ) {
        (Some(b), Some(e)) if b < e => body[b + 1..e].iter().filter_map(|l| acceptance_line(l)).collect(),
        _ => Vec::new(),
    };
    let acceptance = criteria.iter().map(|(_, t)| t.clone()).collect();
    Ok(CardFile {
        id,
        title,
        status,
        labels,
        description,
        acceptance,
        crlf: text.contains("\r\n"),
        notes,
        body: under,
        criteria,
    })
}

fn acceptance_line(l: &str) -> Option<(bool, String)> {
    let rest = l.trim().strip_prefix("- [")?;
    let done = rest.get(..1).is_some_and(|c| !c.trim().is_empty());
    let rest = rest.get(2..)?.trim_start();
    let rest = match rest.strip_prefix('#') {
        Some(r) => r.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start(),
        None => rest,
    };
    Some((done, rest.to_owned()))
}

/// A board config's `task_prefix` and its done status (the last of `statuses`).
pub fn config(text: &str) -> Result<(String, String), BoardError> {
    let get = |key: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{key}:")).map(str::trim));
    let prefix = get("task_prefix")
        .map(scalar)
        .filter(|p| !p.is_empty())
        .ok_or(BoardError::Malformed("the board config has no `task_prefix`"))?;
    let statuses = get("statuses").ok_or(BoardError::Malformed("the board config has no `statuses`"))?;
    let list = statuses
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(BoardError::Malformed("`statuses` isn't a list"))?;
    let done =
        list.split(',').map(scalar).rfind(|s| !s.is_empty()).ok_or(BoardError::Malformed("`statuses` is empty"))?;
    Ok((prefix, done))
}

/// The number of an id (`EX-583.1` → 583), when it is on the prefix.
pub fn number(prefix: &str, id: &str) -> Option<u64> {
    let lower = id.to_lowercase();
    let rest = lower.strip_prefix(&format!("{}-", prefix.to_lowercase()))?;
    rest.split('.').next()?.parse().ok()
}

/// The next id on a board: its prefix and the next number above every id the names hold.
pub fn next_id(prefix: &str, ids: &[String]) -> String {
    let top = ids.iter().filter_map(|i| number(prefix, i)).max().unwrap_or(0);
    format!("{}-{}", prefix.to_lowercase(), top + 1)
}

/// A card's id: letters, a dash, a number, and sub-numbers (`EX-583.1`). It names the card's
/// clone, room and record, so nothing else is taken (R14).
pub fn valid_id(id: &str) -> bool {
    let Some((prefix, rest)) = id.split_once('-') else { return false };
    !prefix.is_empty()
        && prefix.bytes().all(|b| b.is_ascii_alphabetic())
        && rest.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
