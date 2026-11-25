//! T1.47: the source audit: what only its own module may do, and what `core` may never name.

use std::path::Path;

use crate::rust::{self, Seen};
use crate::walk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    ProcessStart,
    ProcessType,
    Sqlite,
    Http,
    Clock,
    Unsafe,
}

impl Kind {
    fn what(self) -> &'static str {
        match self {
            Self::ProcessStart => "a process start",
            Self::ProcessType => "a process type",
            Self::Sqlite => "rusqlite",
            Self::Http => "the HTTP client",
            Self::Clock => "a clock read",
            Self::Unsafe => "unsafe",
        }
    }

    fn allowed(self) -> &'static [&'static str] {
        match self {
            Self::ProcessStart => &["crates/engine/src/jobs/"],
            Self::ProcessType => &["crates/engine/src/jobs/", "crates/sys/src/"],
            Self::Sqlite => &["crates/engine/src/store/"],
            Self::Http => &["crates/engine/src/bitbucket/", "crates/engine/src/azure.rs"],
            Self::Clock => &["crates/engine/src/clock.rs"],
            Self::Unsafe => &["crates/sys/src/"],
        }
    }
}

const HTTP: &[&str] = &["ureq", "reqwest", "hyper", "isahc", "attohttpc", "curl"];

fn starts(p: &str, root: &str) -> bool {
    p == root || p.starts_with(&format!("{root}::"))
}

fn kinds(seen: &Seen) -> Vec<Kind> {
    let mut out = Vec::new();
    match seen {
        Seen::Path(p) => {
            if p.ends_with("Command::new") {
                out.push(Kind::ProcessStart);
            }
            if p.ends_with("process::Command") || p.ends_with("process::Command::new") {
                out.push(Kind::ProcessType);
            }
            if starts(p, "rusqlite") {
                out.push(Kind::Sqlite);
            }
            if HTTP.iter().any(|h| starts(p, h)) {
                out.push(Kind::Http);
            }
            if p.ends_with("SystemTime::now") || p.ends_with("Instant::now") || starts(p, "chrono") {
                out.push(Kind::Clock);
            }
        }
        Seen::Method(m) => match m.as_str() {
            "spawn" | "exec" => out.push(Kind::ProcessStart),
            "elapsed" => out.push(Kind::Clock),
            _ => {}
        },
        Seen::Unsafe => out.push(Kind::Unsafe),
        Seen::Tokens(t) => {
            if t.contains("Command::new") {
                out.push(Kind::ProcessStart);
            }
            if t.contains("SystemTime::now") || t.contains("Instant::now") || t.contains(".elapsed()") {
                out.push(Kind::Clock);
            }
            if t.contains("rusqlite::") {
                out.push(Kind::Sqlite);
            }
            if t.contains("unsafe{") {
                out.push(Kind::Unsafe);
            }
        }
        Seen::Async => {}
    }
    out
}

fn core_forbidden(seen: &Seen) -> Option<String> {
    match seen {
        Seen::Path(p) => {
            for m in ["fs", "net", "process", "thread"] {
                if starts(p, &format!("std::{m}")) {
                    return Some(format!("std::{m}"));
                }
            }
            ["tokio", "async_std", "smol", "rand", "getrandom"].iter().find(|c| starts(p, c)).map(|c| (*c).to_owned())
        }
        Seen::Async => Some("async".into()),
        Seen::Tokens(t) => ["std::fs", "std::net", "std::process", "std::thread"]
            .iter()
            .find(|m| t.contains(*m))
            .map(|m| (*m).to_owned()),
        Seen::Method(_) | Seen::Unsafe => None,
    }
}

// Every crate but the test tools (`fakes`, `xtask`), no tests.
fn audited(root: &Path) -> Vec<(String, String)> {
    walk::rust_files(&root.join("crates"))
        .into_iter()
        .filter_map(|f| {
            let rel = walk::rel(root, &f);
            let skip = rel.starts_with("crates/fakes/")
                || rel.starts_with("crates/xtask/")
                || walk::is_test_file(Path::new(&rel))
                || rel.ends_with("/build.rs");
            if skip {
                return None;
            }
            Some((rel, std::fs::read_to_string(&f).ok()?))
        })
        .collect()
}

/// Every problem found.
pub fn check(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for (rel, text) in audited(root) {
        let seen = match rust::scan(&text) {
            Ok(s) => s,
            Err(e) => {
                out.push(format!("{rel}: can't be read as Rust: {e}"));
                continue;
            }
        };
        for (line, s) in &seen {
            for k in kinds(s) {
                if !k.allowed().iter().any(|a| rel.starts_with(a)) {
                    out.push(format!("{rel}:{line}: {} outside its module", k.what()));
                }
            }
            if rel.starts_with("crates/core/")
                && let Some(what) = core_forbidden(s)
            {
                out.push(format!("{rel}:{line}: core names {what}"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
