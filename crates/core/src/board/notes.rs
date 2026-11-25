//! The dated notes on top of a card's description (`<who> 14.10: …`): this writer indents a note's lines past its
//! first; an earlier writer's multi-line note is told by the lines the factory recorded writing.

use factory_types::card::BoardCard;
use factory_types::decisions;

use crate::state::State;

/// A note the factory wrote into a card's file, as its record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Its lines as written, the first one dated.
    pub lines: Vec<String>,
    /// The board file it went into, where the card then was.
    pub file: String,
    /// Where that write moved the file: a close's, else the file itself or nothing.
    pub dest: String,
    /// When its action was made: which of two notes alike it wrote.
    pub made: i64,
}

impl Written {
    /// Whether it went into this card's file, in whichever folder of the board the file then was.
    pub fn of(&self, file: &str) -> bool {
        let name = super::writes::name(file);
        super::writes::name(&self.file) == name || super::writes::name(&self.dest) == name
    }

    // Whether the engine before fac-374 took it for this path's, by the path alone.
    fn at(&self, file: &str) -> bool {
        self.file == file || self.dest == file
    }
}

/// The notes at the top of a description's lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    /// How many lines they take.
    pub end: usize,
    /// Each note, newest first, its lines joined.
    pub notes: Vec<String>,
    /// The last note, when no one record matches it whole: where it ends isn't known.
    pub unknown: Option<String>,
}

/// A dated note's first line: its author, then the date and a colon.
pub fn is_note(l: &str) -> bool {
    let Some((who, rest)) = l.split_once(' ') else { return false };
    let b = rest.as_bytes();
    !who.is_empty()
        && b.len() >= 6
        && b[..2].iter().chain(&b[3..5]).all(u8::is_ascii_digit)
        && b[2] == b'.'
        && b[5] == b':'
}

// The lines a record takes at `at`, its later lines as they were written or indented, when
// nothing indented carries the note on past them.
fn span<S: AsRef<str>>(lines: &[S], at: usize, w: &Written) -> Option<usize> {
    let line = |i: usize| lines.get(i).map(AsRef::as_ref);
    let rest = w.lines.get(1..)?;
    let as_written = rest.iter().enumerate().all(|(i, r)| line(at + 1 + i) == Some(r.as_str()));
    let indented = rest.iter().enumerate().all(|(i, r)| line(at + 1 + i) == Some(format!("  {r}").as_str()));
    let past = line(at + 1 + rest.len()).is_some_and(|l| l.starts_with("  "));
    ((as_written || indented) && !past).then_some(1 + rest.len())
}

// The records whose first line opens the note at `at` and whose lines take it whole.
fn matching<'a, S: AsRef<str>>(
    lines: &'a [S],
    at: usize,
    written: &'a [&'a Written],
) -> impl Iterator<Item = (&'a Written, usize)> {
    let first = lines.get(at).map(AsRef::as_ref);
    written
        .iter()
        .filter(move |w| w.lines.first().map(String::as_str) == first)
        .filter_map(move |w| span(lines, at, w).map(|len| (*w, len)))
}

/// A note is as long as its card's record that matches it whole says, or of several the one as new as it is among notes
/// opening alike; else as its indented lines say, and unknown. An older withdrawal keeps the decision it meant.
pub fn split<S: AsRef<str>>(lines: &[S], written: &[Written]) -> Notes {
    let all: Vec<&Written> = written.iter().collect();
    let (mut n, starts) = cut(lines, &all, true);
    for i in 0..n.notes.len() {
        if let Some(note) = retargeted(lines, &all, &n, &starts, i) {
            n.notes[i] = note;
        }
    }
    n
}

// The notes, and the line each starts at. Records that disagree are paired with the notes in order only when `paired`:
// the engine before fac-374 took such a note as its indented lines say.
fn cut<S: AsRef<str>>(lines: &[S], written: &[&Written], paired: bool) -> (Notes, Vec<usize>) {
    let line = |i: usize| lines.get(i).map(AsRef::as_ref);
    let (mut n, mut starts) = (Notes::default(), Vec::new());
    while line(n.end).is_some_and(is_note) {
        let mut spans: Vec<usize> = matching(lines, n.end, written).map(|(_, len)| len).collect();
        spans.sort_unstable();
        spans.dedup();
        if paired && spans.len() > 1 {
            spans = in_order(lines, written, &starts, n.end).into_iter().collect();
        }
        let indented = (n.end + 1..).take_while(|&i| line(i).is_some_and(|l| l.starts_with("  "))).count();
        let (len, known) = match spans.as_slice() {
            [one] => (*one, true),
            _ => (1 + indented, false),
        };
        let note: Vec<&str> =
            (n.end..n.end + len).filter_map(line).map(|l| l.strip_prefix("  ").unwrap_or(l)).collect();
        let note = note.join("\n");
        n.unknown = (!known).then(|| note.clone());
        n.notes.push(note);
        starts.push(n.end);
        n.end += len;
    }
    (n, starts)
}

// The lines taken at `at` by the record as new among those opening with its line as that note is among the notes above
// opening alike: notes alike went on top of each other, so the newest is the highest.
fn in_order<S: AsRef<str>>(lines: &[S], written: &[&Written], starts: &[usize], at: usize) -> Option<usize> {
    let first = lines.get(at).map(AsRef::as_ref);
    let above = starts.iter().filter(|&&s| lines.get(s).map(AsRef::as_ref) == first).count();
    let mut alike: Vec<&Written> =
        written.iter().copied().filter(|w| w.lines.first().map(String::as_str) == first).collect();
    alike.sort_by_key(|w| std::cmp::Reverse(w.made));
    span(lines, at, alike.get(above)?)
}

// Note `i`, when it withdraws in a form made before fac-374, by number or by words, over the notes under it as its
// command told them apart, by its path's records made before it: it, numbering as these do the decision it meant.
fn retargeted<S: AsRef<str>>(lines: &[S], all: &[&Written], n: &Notes, starts: &[usize], i: usize) -> Option<String> {
    let note = &n.notes[i];
    let (k, words) = decisions::old_withdrawal(note)?;
    let end = starts.get(i + 1).copied().unwrap_or(n.end);
    let own = own_record(lines, all, n, starts, i)?;
    let then: Vec<&Written> = all.iter().copied().filter(|w| w.at(&own.file) && w.made < own.made).collect();
    let (old, old_starts) = cut(lines.get(end..)?, &then, false);
    let j = match k {
        Some(k) => old.notes.len().checked_sub(k).filter(|_| k > 0)?,
        // As the reader then took it: the newest decision under it with these words still standing.
        None => {
            let gone = decisions::withdrawn(&old.notes);
            (0..old.notes.len()).find(|&j| !gone[j] && decisions::decision_words(&old.notes[j]) == Some(words))?
        }
    };
    let at = end + old_starts.get(j)?;
    let m = starts.iter().position(|&s| s == at)?;
    let now = decisions::number(n.notes.len(), m);
    let unchanged = old_starts.iter().map(|s| end + s).eq(starts[i + 1..].iter().copied()) && end + old.end == n.end;
    let moved = match k {
        Some(k) => k != now,
        None => !unchanged,
    };
    let meant = old.notes.get(j).and_then(|d| decisions::decision_words(d)) == Some(words);
    (meant && m > i && moved).then(|| decisions::renumbered(note, now))?
}

// The record that wrote note `i`. Notes alike, the same lines, are each one of the records matching them whole: the
// oldest the oldest, when there are as many of each; else, when they all wrote to one path, the newest.
fn own_record<'a, S: AsRef<str>>(
    lines: &'a [S],
    all: &'a [&'a Written],
    n: &Notes,
    starts: &[usize],
    i: usize,
) -> Option<&'a Written> {
    let raw = |j: usize| -> Vec<&str> {
        let end = starts.get(j + 1).copied().unwrap_or(n.end);
        starts.get(j).and_then(|&s| lines.get(s..end)).unwrap_or_default().iter().map(AsRef::as_ref).collect()
    };
    let at = starts.get(i).copied()?;
    let len = raw(i).len();
    let mut records: Vec<&Written> = matching(lines, at, all).filter(|&(_, l)| l == len).map(|(w, _)| w).collect();
    records.sort_by_key(|w| w.made);
    let alike: Vec<usize> = (0..starts.len()).filter(|&j| raw(j) == raw(i)).collect();
    let older = alike.iter().filter(|&&j| j > i).count();
    let own = if records.len() == alike.len() {
        records.get(older)
    } else {
        records.last().filter(|f| records.iter().all(|w| w.file == f.file))
    };
    own.copied()
}

/// A card's description apart from its dated notes: the notes, newest first, and the text under them. `board`'s records
/// tell where each note ends; its index may be empty before the board is read.
pub(crate) fn apart(state: &State, board: Option<&BoardCard>, description: &str) -> (Vec<String>, String) {
    let written = board.map(|b| super::writes::written(state, &b.project, &b.file)).unwrap_or_default();
    let lines: Vec<&str> = description.lines().collect();
    let n = split(&lines, &written);
    (n.notes, lines.get(n.end..).unwrap_or_default().join("\n").trim().to_owned())
}
