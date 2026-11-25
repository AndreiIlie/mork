//! T1.48: the size caps, the workspace's lints, and every `#[allow]`'s `// why:` line (fac-2).

use std::path::Path;

use crate::walk;

/// The markers 1.12.4 forbids, built so this file doesn't hold them.
pub fn markers() -> [String; 3] {
    [["TO", "DO"].concat(), ["FIX", "ME"].concat(), ["X", "XX"].concat()]
}

/// Every problem found.
pub fn check(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for file in walk::rust_files(&root.join("crates")) {
        let rel = walk::rel(root, &file);
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let relp = Path::new(&rel);
        if !walk::is_test_file(relp) {
            let n = walk::non_test_lines(&text).len();
            if n > 500 {
                problems.push(format!("{rel}: {n} lines, over 500"));
            }
        }
        problems.extend(markers_in(&rel, &text));
        problems.extend(allows_without_why(&rel, &text));
    }
    problems.extend(lints(root));
    problems
}

fn markers_in(rel: &str, text: &str) -> Vec<String> {
    let [todo, fixme, xxx] = markers();
    let carded = |l: &str| crate::comments::has_card_id(l);
    let found = |l: &str| {
        [(&todo, carded(l)), (&fixme, carded(l)), (&xxx, false)]
            .into_iter()
            .find(|(m, allowed)| !allowed && contains_word(l, m))
            .map(|(m, _)| m.clone())
    };
    text.lines().enumerate().filter_map(|(i, l)| found(l).map(|m| format!("{rel}:{}: {m}", i + 1))).collect()
}

pub(crate) fn contains_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

fn allows_without_why(rel: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("#[allow(") || t.starts_with("#![allow(") {
            // The comment block right above the attribute holds a `// why:` line.
            let why = lines[..i]
                .iter()
                .rev()
                .map(|s| s.trim_start())
                .take_while(|s| s.starts_with("//"))
                .any(|s| s.starts_with("// why:"));
            if !why {
                out.push(format!("{rel}:{}: #[allow] without a `// why:` line above it", i + 1));
            }
        }
    }
    out
}

/// The lints of 1.12.4, set once in `[workspace.lints]`, and the 60-line and complexity-15 thresholds.
pub fn lints(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let Ok(doc) = manifest.parse::<toml::Table>() else { return vec!["Cargo.toml: unreadable".into()] };
    let clippy = doc
        .get("workspace")
        .and_then(|w| w.get("lints"))
        .and_then(|l| l.get("clippy"))
        .and_then(|c| c.as_table())
        .cloned()
        .unwrap_or_default();
    for lint in
        ["too_many_lines", "cognitive_complexity", "unwrap_used", "expect_used", "panic", "todo", "unimplemented"]
    {
        let level = clippy
            .get(lint)
            .and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.get("level")?.as_str().map(str::to_owned)));
        if !matches!(level.as_deref(), Some("deny" | "forbid")) {
            out.push(format!("Cargo.toml: [workspace.lints.clippy] {lint} isn't denied"));
        }
    }
    let conf = std::fs::read_to_string(root.join("clippy.toml")).unwrap_or_default();
    let conf = conf.parse::<toml::Table>().unwrap_or_default();
    for (key, limit) in [("too-many-lines-threshold", 60), ("cognitive-complexity-threshold", 15)] {
        if conf.get(key).and_then(toml::Value::as_integer) != Some(limit) {
            out.push(format!("clippy.toml: {key} isn't {limit}"));
        }
    }
    for (dir, _) in crate::CRATES.iter().chain([&("xtask", "xtask")]) {
        let m = std::fs::read_to_string(root.join("crates").join(dir).join("Cargo.toml")).unwrap_or_default();
        let ok = m.parse::<toml::Table>().ok().and_then(|t| t.get("lints")?.get("workspace")?.as_bool()) == Some(true);
        if !ok {
            out.push(format!("crates/{dir}/Cargo.toml: doesn't take [lints] workspace = true"));
        }
    }
    out
}
