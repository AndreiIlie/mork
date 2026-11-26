// A rewrite under an existing memory file's name is likelier meant than a new file (fac-201).

use std::collections::BTreeSet;

use mork_types::memory::MemoryFile;

pub(super) fn close_file<'a>(
    files: impl Iterator<Item = &'a MemoryFile>,
    name: &str,
    description: &str,
) -> Option<&'a MemoryFile> {
    let new = words(name, description);
    files.into_iter().find(|f| {
        let old = words(&f.name, &f.description);
        let overlap = new.intersection(&old).count();
        // Three shared words and a third of the smaller set catch renamed lessons while
        // tolerating incidental vocabulary (fac-201).
        overlap >= 3 && overlap * 3 >= new.len().min(old.len())
    })
}

fn words(name: &str, description: &str) -> BTreeSet<String> {
    let text = format!("{name} {description}").to_lowercase();
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            word.chars().count() > 3
                && !matches!(*word, "that" | "this" | "with" | "from" | "when" | "then" | "have" | "only" | "into")
        })
        .map(str::to_owned)
        .collect()
}
