use super::file::{front, valid_id};
use super::notes::{self, Written};
use super::{BEGIN, BoardError, END};

fn slug(title: &str) -> String {
    let kept: String = title.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-').collect();
    kept.split_whitespace().collect::<Vec<_>>().join("-")
}

/// A value written on one line of the card: its field named when it can't be.
pub fn one_line(field: &'static str, v: &str) -> Result<(), BoardError> {
    if v.contains(['\n', '\r']) {
        return Err(BoardError::Field(field, "is one line: it holds a line break"));
    }
    if v.trim().is_empty() {
        return Err(BoardError::Field(field, "is empty"));
    }
    no_markers(field, v)
}

/// Text that would end or open a section of the card's file.
pub fn no_markers(field: &'static str, text: &str) -> Result<(), BoardError> {
    if text.contains("<!-- SECTION:") || text.contains("<!-- AC:") {
        return Err(BoardError::Field(field, "holds the section markers"));
    }
    Ok(())
}

fn checked_text(text: &str) -> Result<(), BoardError> {
    if text.contains(BEGIN) || text.contains(END) || text.contains("<!-- AC:") {
        return Err(BoardError::Refused("the text holds the section markers"));
    }
    Ok(())
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A new card's file name and file (4.18.2).
pub fn new_card(id: &str, title: &str, description: &str, created: &str) -> Result<(String, String), BoardError> {
    if !valid_id(id) {
        return Err(BoardError::Refused("the id isn't `<prefix>-<n>`"));
    }
    one_line("title", title)?;
    checked_text(description)?;
    let name = format!("{} - {}.md", id.to_lowercase(), slug(title));
    let text = format!(
        "---\nid: {id}\ntitle: {}\nstatus: Backlog\nassignee: []\ncreated_date: '{created}'\nlabels: []\ndependencies: []\n---\n\n## Description\n\n{BEGIN}\n{}\n{END}\n",
        quoted(title),
        description.trim()
    );
    Ok((name, text))
}

fn split(text: &str) -> (Vec<String>, &'static str) {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    (text.lines().map(|l| l.trim_end_matches('\r').to_owned()).collect(), nl)
}

fn join(lines: &[String], nl: &str, trailing: bool) -> String {
    let mut s = lines.join(nl);
    if trailing {
        s.push_str(nl);
    }
    s
}

fn set_front(lines: &mut Vec<String>, key: &str, value: &str) -> Result<(), BoardError> {
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let end = front(&refs)?;
    let prefix = format!("{key}:");
    let at = (1..end).find(|&i| lines[i].starts_with(&prefix));
    match at {
        Some(i) => {
            // A list value spans the indented lines after it: they go with it.
            let mut j = i + 1;
            while j < end && lines[j].starts_with("  ") {
                j += 1;
            }
            lines.splice(i..j, [format!("{key}: {value}")]);
        }
        None => lines.insert(end, format!("{key}: {value}")),
    }
    Ok(())
}

/// A dated note added at the top of the description (4.18.3), keeping every line and the line
/// endings. A note's lines past its first are indented, so they read as the note's.
pub fn add_note(text: &str, note: &str, updated: &str) -> Result<String, BoardError> {
    checked_text(note)?;
    let (mut lines, nl) = split(text);
    put_note(&mut lines, note)?;
    set_front(&mut lines, "updated_date", &format!("'{updated}'"))?;
    Ok(join(&lines, nl, text.ends_with('\n')))
}

fn section_at(lines: &[String]) -> Result<(usize, usize), BoardError> {
    let begin = lines.iter().position(|l| l.trim() == BEGIN);
    let end = lines.iter().position(|l| l.trim() == END);
    match (begin, end) {
        (Some(b), Some(e)) if b < e => Ok((b, e)),
        _ => Err(BoardError::Malformed("the card has no description section")),
    }
}

fn put_note(lines: &mut Vec<String>, note: &str) -> Result<(), BoardError> {
    let (at, _) = section_at(lines)?;
    let note_lines = note.lines().enumerate().map(|(i, l)| if i == 0 { l.to_owned() } else { format!("  {l}") });
    lines.splice(at + 1..at + 1, note_lines.collect::<Vec<_>>());
    Ok(())
}

// The dated notes on top of the description are kept; refused when what it would replace may hold
// a note: one no record says the end of, or a dated line under it.
fn put_description(lines: &mut Vec<String>, description: &str, written: &[Written]) -> Result<(), BoardError> {
    let (b, e) = section_at(lines).map_err(|_| BoardError::Field("description", "has no section in this card"))?;
    let n = notes::split(&lines[b + 1..e], written);
    let under = &lines[b + 1 + n.end..e];
    if n.unknown.is_some() && under.iter().any(|l| !l.trim().is_empty()) {
        return Err(BoardError::Field(
            "description",
            "can't be replaced: no record of Mork's says where the note on top of it ends; add what changed as a note instead (`card.change --note`, or `--decision` for the operator's)",
        ));
    }
    if under.iter().any(|l| notes::is_note(l)) {
        return Err(BoardError::Field(
            "description",
            "can't be replaced: it holds a dated note a replacement would delete; add what changed as a note instead (`card.change --note`, or `--decision` for the operator's)",
        ));
    }
    let new = description.trim().lines().map(str::to_owned).collect::<Vec<_>>();
    lines.splice(b + 1 + n.end..e, new);
    Ok(())
}

/// What a change sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub title: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub parent: Option<String>,
    /// Criteria to add, one per line.
    pub acceptance: Option<String>,
    /// The description under the notes, replaced whole.
    pub description: Option<String>,
    /// A dated note's line, put on top once the description is replaced.
    pub note: Option<String>,
}

/// Every value of a change checked before any is written, each refusal naming its field.
pub fn check_fields(f: &Fields) -> Result<(), BoardError> {
    let lines = [("title", &f.title), ("status", &f.status), ("priority", &f.priority), ("parent", &f.parent)];
    for (field, v) in lines {
        if let Some(v) = v {
            one_line(field, v)?;
        }
    }
    for l in f.labels.iter().flatten() {
        one_line("labels", l)?;
        if l.contains([',', '[', ']']) {
            return Err(BoardError::Field("labels", "holds a label with a comma or a bracket"));
        }
    }
    if let Some(d) = &f.description {
        if d.trim().is_empty() {
            return Err(BoardError::Field("description", "is empty"));
        }
        no_markers("description", d)?;
    }
    if let Some(n) = &f.note {
        no_markers("note", n)?;
    }
    if let Some(a) = &f.acceptance {
        no_markers("acceptance", a)?;
        if criteria_of(a).next().is_none() {
            return Err(BoardError::Field("acceptance", "holds no criterion: give one per line"));
        }
    }
    Ok(())
}

fn criteria_of(a: &str) -> impl Iterator<Item = &str> {
    a.lines().map(str::trim).filter(|l| !l.is_empty())
}

/// A change written whole: the description, then the note on top, then the front matter and
/// the criteria, and `updated_date` once. `written` tells the notes the description keeps.
pub fn set_fields(text: &str, f: &Fields, written: &[Written], updated: &str) -> Result<String, BoardError> {
    check_fields(f)?;
    let (mut lines, nl) = split(text);
    if let Some(d) = &f.description {
        put_description(&mut lines, d, written)?;
    }
    if let Some(n) = &f.note {
        put_note(&mut lines, n).map_err(|_| BoardError::Field("note", "has no description to go on top of"))?;
    }
    if let Some(t) = &f.title {
        set_front(&mut lines, "title", &quoted(t))?;
    }
    for (key, v) in [("status", &f.status), ("priority", &f.priority), ("parent_task_id", &f.parent)] {
        if let Some(v) = v {
            set_front(&mut lines, key, v)?;
        }
    }
    if let Some(labels) = &f.labels {
        let v = if labels.is_empty() { "[]".to_owned() } else { format!("[{}]", labels.join(", ")) };
        set_front(&mut lines, "labels", &v)?;
    }
    for a in f.acceptance.as_deref().map(criteria_of).into_iter().flatten() {
        add_acceptance(&mut lines, a);
    }
    set_front(&mut lines, "updated_date", &format!("'{updated}'"))?;
    Ok(join(&lines, nl, text.ends_with('\n')))
}

fn add_acceptance(lines: &mut Vec<String>, a: &str) {
    let begin = lines.iter().position(|l| l.trim() == "<!-- AC:BEGIN -->");
    match (begin, lines.iter().position(|l| l.trim() == "<!-- AC:END -->")) {
        (Some(b), Some(end)) if b < end => {
            let n = lines[b + 1..end].iter().filter(|l| l.trim_start().starts_with("- [")).count() + 1;
            lines.insert(end, format!("- [ ] #{n} {a}"));
        }
        _ => {
            lines.extend(["".to_owned(), "## Acceptance Criteria".to_owned(), "<!-- AC:BEGIN -->".to_owned()]);
            lines.push(format!("- [ ] #1 {a}"));
            lines.push("<!-- AC:END -->".to_owned());
        }
    }
}

/// A card closed (4.18.4): its status, and a closing note at the top of its description.
pub fn close(text: &str, status: &str, note: &str, updated: &str) -> Result<String, BoardError> {
    let noted = add_note(text, note, updated)?;
    set_fields(&noted, &Fields { status: Some(status.to_owned()), ..Fields::default() }, &[], updated)
}
