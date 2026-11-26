//! Typechecking, building, and class-rule scanning for `web/` (fac-672, fac-695).

mod classes;
mod lexer;
mod scan;

use std::path::Path;
use std::process::{Command, Output};
use std::time::SystemTime;

pub use classes::{check_classes, scan_classes};

/// A single located finding from `tsc --noEmit --pretty false` or the web class check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TscError {
    pub file: String,
    pub line: u64,
    pub summary: String,
    pub proof: String,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Whether `web/` needs `npm ci`: `node_modules` missing or `package-lock.json` newer than its copy.
pub fn needs_install(web_dir: &Path) -> bool {
    if !web_dir.join("node_modules").is_dir() {
        return true;
    }
    let lock = mtime(&web_dir.join("package-lock.json"));
    let installed = mtime(&web_dir.join("node_modules/.package-lock.json"));
    match (lock, installed) {
        (Some(lock), Some(installed)) => lock > installed,
        _ => true,
    }
}

/// Builds an `npm` command rooted in `web_dir`.
pub fn npm(web_dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("npm");
    c.current_dir(web_dir).args(args);
    c
}

fn capture(web_dir: &Path, args: &[&str]) -> Result<Output, String> {
    npm(web_dir, args).output().map_err(|e| format!("couldn't run npm: {e}"))
}

fn combined(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    format!("{stdout}\n{stderr}").trim().to_owned()
}

/// Runs install (when needed), class-rule checks, typecheck, and build for `xtask checks`.
pub fn check_findings(root: &Path) -> Result<Vec<TscError>, String> {
    let web = root.join("web");
    if needs_install(&web) {
        let out = capture(&web, &["ci"])?;
        if !out.status.success() {
            return Err(format!("npm ci failed ({})\n{}", out.status, combined(&out)));
        }
    }
    let mut findings = check_classes(root);
    let tc = capture(&web, &["run", "typecheck"])?;
    let text = combined(&tc);
    let errors = parse_tsc(&text);
    if !tc.status.success() {
        if errors.is_empty() {
            return Err(format!("npm run typecheck failed ({}) without located diagnostics\n{text}", tc.status));
        }
        findings.extend(errors);
        return Ok(findings);
    }
    findings.extend(errors);
    if !findings.is_empty() {
        return Ok(findings);
    }
    let build = capture(&web, &["run", "build"])?;
    if !build.status.success() {
        return Err(format!("npm run build failed ({})\n{}", build.status, combined(&build)));
    }
    Ok(Vec::new())
}

fn parse_header(line: &str) -> Option<(String, u64, String)> {
    let (loc, rest) = line.split_once("): ")?;
    if !rest.starts_with("error TS") {
        return None;
    }
    let (file, coords) = loc.rsplit_once('(')?;
    let (row, col) = coords.split_once(',')?;
    if file.is_empty() || col.is_empty() || !col.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let line_no = row.parse::<u64>().ok()?;
    Some((file.trim().replace('\\', "/"), line_no, rest.trim().to_owned()))
}

/// Parses `file(line,col): error TS…` lines and indented continuation lines from `tsc --noEmit --pretty false`.
pub fn parse_tsc(output: &str) -> Vec<TscError> {
    let mut out: Vec<TscError> = Vec::new();
    for line in output.lines() {
        if let Some((file, line_no, summary)) = parse_header(line) {
            out.push(TscError { file, line: line_no, summary, proof: line.to_owned() });
        } else if line.starts_with("  ")
            && let Some(last) = out.last_mut()
        {
            last.proof.push('\n');
            last.proof.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tsc_single_and_multiline_errors_and_ignores_npm_banner() {
        let output = "\
> mork-web@ typecheck
> tsc --noEmit --pretty false

src/App.tsx(12,5): error TS2322: Type 'number' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'.
src/bridge.ts(40,10): error TS2304: Cannot find name 'foo'.
";
        let got = parse_tsc(output);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].file, "src/App.tsx");
        assert_eq!(got[0].line, 12);
        assert_eq!(got[0].summary, "error TS2322: Type 'number' is not assignable to type 'string'.");
        assert_eq!(
            got[0].proof,
            "src/App.tsx(12,5): error TS2322: Type 'number' is not assignable to type 'string'.\n  Type 'number' is not assignable to type 'string'."
        );
        assert_eq!(got[1].file, "src/bridge.ts");
        assert_eq!(got[1].line, 40);
        assert_eq!(got[1].summary, "error TS2304: Cannot find name 'foo'.");
    }

    #[test]
    fn ignores_unlocated_tsc_errors() {
        let output = "error TS5023: Unknown compiler option '--bogus'.\n";
        assert!(parse_tsc(output).is_empty());
    }
}
