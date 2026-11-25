// Only correctness and performance checks belong in a colleague's round (fac-220).

use std::fmt::Write as _;

use serde_json::Value;

use super::style_rules;

/// The report given to a round: our card's PR keeps it all; a colleague's loses house findings.
pub fn output(report: &Value, n: usize, colleague: bool) -> String {
    if !colleague {
        return format!("The checks found {n}:\n{report}");
    }
    let mut report = report.clone();
    let mut dropped = [0; 3];
    let Some(findings) = report["findings"].as_array_mut() else {
        return format!("The checks found {n}:\n{report}");
    };
    findings.retain(|finding| match kind(finding) {
        Some(i) => {
            dropped[i] += 1;
            false
        }
        None => true,
    });
    let retained = findings.len();
    let mut out = format!("The checks retain {retained} findings:\n{report}");
    for (label, count) in ["ESLint style", "copies", "words"].iter().zip(dropped) {
        let _ = write!(out, "\nDropped {label} findings: {count}.");
    }
    out
}

// Unknown tools and rule ids may report real bugs: only known house findings are omitted.
fn kind(finding: &Value) -> Option<usize> {
    if finding["tool"].as_str()? != "lint" {
        return None;
    }
    let summary = finding["summary"].as_str()?;
    if summary.starts_with("new words finding ") {
        return Some(2);
    }
    let (rule, _) = summary.split_once(":")?;
    let rule = rule.split_whitespace().last()?;
    if matches!(rule, "jscpd/copied-block" | "house/copied-one-line") {
        return Some(1);
    }
    style(rule).then_some(0)
}

// Plugin style rules come from the checks tool's metadata, not guessed from their names.
fn style(rule: &str) -> bool {
    style_rules::CORE.contains(&rule)
        || rule.starts_with("@stylistic/")
        || matches!(
            rule,
            "slop/max-comment-length"
                | "slop/no-trivial-functions"
                | "unicorn/name-replacements"
                | "unicorn/consistent-function-scoping"
                | "unicorn/no-useless-undefined"
                | "unicorn/filename-case"
        )
}
