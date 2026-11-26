//! A failed suite's output as the train tells it (fac-144): what failed, read from its whole
//! output, before its tail, and where the whole of it is kept.

use mork_types::work::Failing;

// A run with many failures must still fit a foreman's message: the full log has the rest.
const BLOCK: usize = 20;
const MOST: usize = 120;

/// The run's output for its foreman: its `failing` tests by file, its failures with their panic or
/// error lines read from `whole`, then `tail` as the run kept it, then the path of its `log`.
pub fn told(whole: &str, tail: &str, log: &str, failing: Option<&[Failing]>) -> String {
    let mut failed = failures(whole);
    let mut parts = Vec::new();
    if let Some(f) = failing.filter(|f| !f.is_empty()) {
        parts.push(super::failing::listed(f));
    }
    if !failed.is_empty() {
        if failed.len() > MOST {
            let more = failed.len() - MOST;
            failed.truncate(MOST);
            failed.push(format!("(cut here: {more} more lines of failures are in the full log)"));
        }
        parts.push(format!("What failed:\n{}", failed.join("\n")));
    }
    parts.push(format!("Its last lines:\n{tail}"));
    parts.push(format!("The full log: {log}"));
    parts.join("\n\n")
}

// xtask prints a binary's verdict before its output but a step's after it, so a failed test is read
// below its verdict and a step's errors above it, back to the step's start.
fn failures(whole: &str) -> Vec<String> {
    let lines: Vec<&str> = whole.lines().collect();
    let (mut kept, mut start, mut in_failed) = (Vec::new(), None::<usize>, false);
    let mut i = 0;
    while let Some(&l) = lines.get(i) {
        i += 1;
        if let Some(said) = l.strip_prefix("xtask: ") {
            // A test's result told as it landed is told again under its binary's verdict (fac-697).
            if said.split_once(": test ").is_some_and(|(_, t)| t.contains(" ... ")) {
                continue;
            }
            in_failed = said.contains("FAILED");
            if in_failed {
                kept.push(l.to_owned());
                let what = said.split(": FAILED").next().unwrap_or(said);
                if let Some(s) =
                    start.filter(|s| lines.get(*s).is_some_and(|x| x.strip_prefix("xtask: ") == Some(what)))
                {
                    kept.extend(errors(lines.get(s + 1..i - 1).unwrap_or_default()));
                }
            }
            start = Some(i - 1);
        } else if in_failed && l.starts_with("test ") && l.ends_with(" ... FAILED") {
            kept.push(l.to_owned());
        } else if in_failed && l.starts_with("---- ") && l.ends_with(" ----") {
            let (block, used) = block(lines.get(i..).unwrap_or_default());
            kept.push(l.to_owned());
            kept.extend(block);
            i += used;
        } else if in_failed && l.contains("panicked at ") {
            let (said, used) = under(lines.get(i - 1..).unwrap_or_default());
            kept.extend(said);
            i += used - 1;
        }
    }
    kept
}

// libtest gathers a test's panic in its stdout block, after every test's verdict line.
fn block(rest: &[&str]) -> (Vec<String>, usize) {
    let end = rest
        .iter()
        .position(|l| {
            l.starts_with("---- ") || *l == "failures:" || l.starts_with("test result:") || l.starts_with("xtask: ")
        })
        .unwrap_or(rest.len());
    let said =
        rest.iter().take(end).filter(|l| !l.trim().is_empty() && !l.starts_with("note: run with `RUST_BACKTRACE"));
    (cap(said.map(|l| (*l).to_owned()).collect()), end)
}

// An error's first line rarely says enough: rustc's span and a panic's message sit under it.
fn errors(lines: &[&str]) -> Vec<String> {
    let mut kept = Vec::new();
    let mut i = 0;
    while let Some(l) = lines.get(i) {
        if l.starts_with("error") || l.contains("panicked at ") {
            let (said, used) = under(lines.get(i..).unwrap_or_default());
            kept.extend(said);
            i += used;
        } else {
            i += 1;
        }
    }
    kept
}

// A binary killed mid-panic leaves no blank line: the next verdict or test line ends the excerpt,
// else the next binary's verdict would be swallowed (fac-144's round 1).
fn under(rest: &[&str]) -> (Vec<String>, usize) {
    let n = rest.iter().skip(1).position(|x| x.trim().is_empty() || ends(x)).map_or(rest.len(), |p| p + 1);
    (cap(rest.iter().take(n).map(|x| (*x).to_owned()).collect()), n.max(1))
}

fn ends(l: &str) -> bool {
    l.starts_with("xtask: ") || l.starts_with("test ")
}

fn cap(mut lines: Vec<String>) -> Vec<String> {
    if lines.len() > BLOCK {
        lines.truncate(BLOCK);
        lines.push("    (cut)".to_owned());
    }
    lines
}
