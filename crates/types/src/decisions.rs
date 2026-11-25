//! A marked decision and its withdrawal (fac-256): both are dated notes, so the history stays on
//! the card and every reader works out from its notes alone which decisions still stand.

/// What a note's text starts with when it marks a decision.
pub const DECISION: &str = "Decision:";
/// What a note's text starts with when it withdraws a decision.
pub const WITHDRAWN: &str = "Withdrawn decision";

/// The number of the note at `index` among `total` notes, newest first: the oldest is #1, and a
/// note keeps its number as newer ones are added on top (fac-347).
pub fn number(total: usize, index: usize) -> usize {
    total - index
}

// A dated note's words after its signature and date, whole.
fn text(note: &str) -> &str {
    note.split_once(": ").map_or(note, |(_, t)| t)
}

/// The words of a marked decision note, without its marker.
pub fn decision_words(note: &str) -> Option<&str> {
    text(note).strip_prefix(DECISION).map(str::trim)
}

/// Whether a dated note is a withdrawal: its words start with the marker.
pub fn is_withdrawal(note: &str) -> bool {
    withdraws(note).is_some()
}

// Which decision a withdrawal takes back: a note by its number, or, in the form older notes
// have, the newest earlier one with exactly these words.
enum Target<'a> {
    Number(usize),
    Words(&'a str),
}

// What a withdrawal puts between its decision's number and words. One made before fac-374 put a colon, its number
// counting the notes as the engine then told a closed card's apart.
const NUMBERED: &str = " - ";

fn withdraws(note: &str) -> Option<Target<'_>> {
    let rest = text(note).strip_prefix(WITHDRAWN)?;
    if let Some(words) = rest.strip_prefix(':') {
        return Some(Target::Words(words.trim()));
    }
    let new = rest.strip_prefix(" #")?.split_once(NUMBERED).and_then(|(n, _)| n.parse().ok());
    new.or_else(|| old_withdrawal(note).and_then(|(n, _)| n)).map(Target::Number)
}

/// A withdrawal in a form made before fac-374: the number it gave its decision, none when it quotes its words alone,
/// and those words.
pub fn old_withdrawal(note: &str) -> Option<(Option<usize>, &str)> {
    let rest = text(note).strip_prefix(WITHDRAWN)?;
    if let Some(words) = rest.strip_prefix(':') {
        return Some((None, words.trim()));
    }
    let (n, words) = rest.strip_prefix(" #")?.split_once(':')?;
    Some((Some(n.parse().ok()?), words.trim()))
}

/// The note text that withdraws decision #`n`, which holds these words.
pub fn withdrawal(n: usize, words: &str) -> String {
    format!("{WITHDRAWN} #{n}{NUMBERED}{words}")
}

/// An old-form withdrawal as it reads numbering its decision #`n`: its signature and words kept.
pub fn renumbered(note: &str, n: usize) -> Option<String> {
    let (_, words) = old_withdrawal(note)?;
    let signed = note.strip_suffix(text(note))?;
    Some(format!("{signed}{WITHDRAWN} #{n}: {words}"))
}

/// For each of these notes, newest first: whether a later note withdrew it. A withdrawal takes
/// back the standing decision it numbers, or an old-form one the newest with its words.
pub fn withdrawn(notes: &[String]) -> Vec<bool> {
    let mut out = vec![false; notes.len()];
    for (i, note) in notes.iter().enumerate().rev() {
        let Some(target) = withdraws(note) else { continue };
        let found = match target {
            Target::Number(n) => (1..=notes.len())
                .contains(&n)
                .then(|| notes.len() - n)
                .filter(|&j| j > i && is_standing(notes, &out, j)),
            Target::Words(w) => {
                (i + 1..notes.len()).find(|&j| is_standing(notes, &out, j) && decision_words(&notes[j]) == Some(w))
            }
        };
        if let Some(j) = found {
            out[j] = true;
        }
    }
    out
}

fn is_standing(notes: &[String], out: &[bool], j: usize) -> bool {
    !out[j] && decision_words(&notes[j]).is_some()
}

/// The decisions that still stand among these notes, newest first.
pub fn standing(notes: &[String]) -> Vec<&String> {
    let gone = withdrawn(notes);
    notes.iter().zip(gone).filter(|(n, g)| !g && decision_words(n).is_some()).map(|(n, _)| n).collect()
}
