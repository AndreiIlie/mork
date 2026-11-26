//! A failed suite's failing tests by file (fac-164): one built-in reader per runner, picked by
//! recognising its output, xtask's runs of libtest binaries and Jest's default reporter.

use std::collections::BTreeMap;

use mork_types::work::Failing;

/// What a Rust binary that failed without naming a failing test, killed or crashed, is listed as.
pub const NO_TEST: &str = "(the binary failed, naming no failing test)";

/// The failing tests `whole` names, each once, by file; none when no reader recognises it.
/// `packages` gives each Rust package's folder in the repository, for its binaries' files.
pub fn read(whole: &str, packages: &BTreeMap<String, String>) -> Option<Vec<Failing>> {
    let plain = unstyled(whole);
    let mut found = jest(&plain).or_else(|| xtask(&plain, packages))?;
    let mut seen = std::collections::BTreeSet::new();
    found.retain(|f| seen.insert(f.clone()));
    Some(found)
}

/// The list as the failure news gives it, each file once with its tests under it.
pub fn listed(failing: &[Failing]) -> String {
    let mut by: BTreeMap<&str, Vec<&Failing>> = BTreeMap::new();
    for f in failing {
        by.entry(f.file.as_str()).or_default().push(f);
    }
    let mut out = vec![format!("Failing tests, by file ({}):", failing.len())];
    for (file, tests) in by {
        out.push(file.to_owned());
        out.extend(tests.into_iter().map(|t| format!("  {}", t.test)));
    }
    out.join("\n")
}

/// `s` without its terminal colours: a runner that colours its output when it thinks a terminal
/// reads it still says the same words.
pub fn unstyled(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

// xtask names a binary `<package> <its source, from the package's folder>`, its verdict printed
// before its output; a failed step that is no binary names no test, but rustdoc names its own.
fn xtask(whole: &str, packages: &BTreeMap<String, String>) -> Option<Vec<Failing>> {
    let mut seen = false;
    let (mut out, mut file, mut named) = (Vec::new(), None::<String>, false);
    for l in whole.lines() {
        if let Some(said) = l.strip_prefix("xtask: ") {
            seen = true;
            if let Some(f) = file.take().filter(|_| !named) {
                out.push(Failing { file: f, test: NO_TEST.to_owned() });
            }
            named = false;
            file = said.split_once(": FAILED").and_then(|(what, _)| binary(what, packages));
        } else if let Some(d) = l.strip_prefix("test ").and_then(|t| t.strip_suffix(" ... FAILED")).and_then(doctest) {
            out.push(d);
        } else if let Some(f) = &file
            && let Some(test) = l.strip_prefix("test ").and_then(|t| t.strip_suffix(" ... FAILED"))
        {
            out.push(Failing { file: f.clone(), test: test.to_owned() });
            named = true;
        }
    }
    if let Some(f) = file.filter(|_| !named) {
        out.push(Failing { file: f, test: NO_TEST.to_owned() });
    }
    seen.then_some(out)
}

// rustdoc's `<path>.rs - <item> (line N)`, or `- (line N)` for a crate's own docs, under its path
// whatever binary was named last.
fn doctest(t: &str) -> Option<Failing> {
    let (path, item) = t.split_once(".rs - ")?;
    (item.starts_with("(line ") || item.contains(" (line "))
        .then(|| Failing { file: format!("{path}.rs"), test: item.to_owned() })
}

// A package the repository doesn't name keeps the package beside its file.
fn binary(what: &str, packages: &BTreeMap<String, String>) -> Option<String> {
    let (package, src) = what.split_once(' ')?;
    if !src.ends_with(".rs") || src.contains(' ') {
        return None;
    }
    Some(match packages.get(package).map(String::as_str) {
        Some("") => src.to_owned(),
        Some(dir) => format!("{dir}/{src}"),
        None => format!("{package} {src}"),
    })
}

// Jest's default reporter heads each file `FAIL <path>`, maybe after a project's name and before
// its time, then each failed test `● Suite › test`; its summary repeats them.
fn jest(whole: &str) -> Option<Vec<Failing>> {
    let mut seen = false;
    let (mut out, mut file) = (Vec::new(), None::<String>);
    for l in whole.lines() {
        let t = l.trim();
        if t.starts_with("Test Suites: ") {
            seen = true;
            file = None;
        } else if let Some(rest) = t.strip_prefix("FAIL ") {
            seen = true;
            file = path(rest);
        } else if t.starts_with("PASS ") || t == "Summary of all failing tests" {
            file = None;
        } else if let (Some(f), Some(test)) = (&file, l.strip_prefix("  ● "))
            && test != "Console"
        {
            out.push(Failing { file: f.clone(), test: test.trim_end().to_owned() });
        }
    }
    seen.then_some(out)
}

/// The test file a Jest `PASS` or `FAIL` line names after its word, without its project's name
/// before it or its time after it.
pub fn path(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let rest = match rest.rfind(" (") {
        Some(i) if rest.ends_with(')') => rest.get(..i).unwrap_or(rest),
        _ => rest,
    };
    rest.split_whitespace().last().map(str::to_owned)
}
