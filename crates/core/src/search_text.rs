const BEFORE: usize = 80;
const AFTER: usize = 220;

// Folded char by char, so a match maps back to the text; a final sigma folds as any other.
fn fold_char(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().map(|l| if l == 'ς' { 'σ' } else { l })
}

pub(crate) fn fold(s: &str) -> String {
    s.chars().flat_map(fold_char).collect()
}

// The text around its first match, on one line.
pub(crate) fn excerpt(text: &str, words: &[String]) -> String {
    let mut hay = String::new();
    let mut starts = Vec::new();
    for (i, c) in text.char_indices() {
        hay.extend(fold_char(c));
        starts.resize(hay.len(), i);
    }
    let first = words.iter().filter_map(|w| hay.find(w.as_str())).min();
    let at = first.and_then(|b| starts.get(b).copied()).unwrap_or(0);
    let (head, tail) = text.split_at(at);
    let from = head.char_indices().rev().nth(BEFORE.saturating_sub(1)).map_or(0, |(i, _)| i);
    let to = tail.char_indices().nth(AFTER).map_or(text.len(), |(i, _)| at + i);
    let mut out = text.get(from..to).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    if from > 0 {
        out.insert(0, '…');
    }
    if to < text.len() {
        out.push('…');
    }
    out
}
