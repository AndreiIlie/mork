//! The internals check (3.10.6), shared by the push and the branch names it accepts (fac-394).

/// The internals check (3.10.6): the first thing a text holds that says how the work was made.
pub fn internals(words: &[String], home_root: &str, text: &str) -> Option<String> {
    for marker in ["Co-Authored-By:", "Generated with", "🤖"] {
        if text.to_lowercase().contains(&marker.to_lowercase()) {
            return Some(format!("`{marker}`"));
        }
    }
    if !home_root.is_empty() && text.contains(home_root) {
        return Some("a path inside the factory's home".into());
    }
    if let Some(name) = agent_name(text) {
        return Some(format!("the agent name `{name}`"));
    }
    words.iter().find(|w| whole(text, w)).map(|w| format!("`{w}`"))
}

// Factory's name and its agents' (fac-95): `Factory` as a name, or `reviewer-1`,
// `researcher-3`, `baseline-1` (a key's `web-1/foreman` is a configured word's).
fn agent_name(text: &str) -> Option<&str> {
    let named = |w: &&str| {
        *w == "Factory"
            || ["reviewer-", "researcher-", "baseline-"].iter().any(|role| {
                w.to_lowercase()
                    .strip_prefix(role)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
    };
    text.split(|c: char| !(c.is_alphanumeric() || c == '-')).find(named)
}

/// Whether the word occurs in any letter case, touching no alphanumeric character.
pub(crate) fn whole(text: &str, word: &str) -> bool {
    let (t, w) = (text.to_lowercase(), word.to_lowercase());
    if w.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = t[from..].find(&w) {
        let at = from + i;
        let before = t[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = t[at + w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = at + w.len().max(1);
    }
    false
}
