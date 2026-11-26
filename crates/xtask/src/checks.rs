mod diagnostics;
mod test_changes;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

struct Report {
    components: Vec<Value>,
    findings: Vec<Value>,
    status: u8,
}

impl Report {
    fn step(&mut self, name: &str, result: Result<Vec<Value>, String>) {
        let status = match result {
            Ok(found) => {
                let status = u8::from(!found.is_empty());
                self.findings.extend(found);
                status
            }
            Err(e) => {
                eprintln!("xtask: {name}: untrusted: {e}");
                2
            }
        };
        self.status = self.status.max(status);
        self.components.push(json!({"name": name, "status": status}));
        eprintln!("xtask: {name}: {}", ["ok", "FAILED", "untrusted"][usize::from(status)]);
    }
}

fn arguments() -> Result<(String, PathBuf), String> {
    let mut args = std::env::args().skip(2);
    let (mut base, mut path) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base" if base.is_none() => base = Some(args.next().ok_or("--base needs a value")?),
            "--json" if path.is_none() => path = Some(PathBuf::from(args.next().ok_or("--json needs a value")?)),
            _ => return Err(format!("unknown or repeated argument: {arg}")),
        }
    }
    Ok((base.ok_or("--base is required")?, path.ok_or("--json is required")?))
}

fn output(mut command: Command) -> Result<std::process::Output, String> {
    command
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", command.get_program().display()))
}

fn child(tool: &str) -> Result<Vec<Value>, String> {
    let mut c = super::cargo();
    match tool {
        "rustfmt" => c.args(["fmt", "--all", "--check"]),
        _ => c.args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--message-format=json",
            "--",
            "-D",
            "warnings",
        ]),
    };
    let out = output(c)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let found = if tool == "rustfmt" {
        diagnostics::rustfmt(&xtask::root(), &format!("{stdout}\n{stderr}"))?
    } else {
        diagnostics::clippy(&xtask::root(), &stdout)?
    };
    let code = out.status.code();
    let expected = if tool == "rustfmt" { 1 } else { 101 };
    if !out.status.success() && (code != Some(expected) || found.is_empty()) {
        return Err(format!("{} without a complete located diagnosis\n{stdout}\n{stderr}", out.status));
    }
    Ok(found)
}

fn problems(root: &Path, texts: Vec<String>, tool: &str) -> Result<Vec<Value>, String> {
    texts.iter().map(|text| diagnostics::problem(root, text, tool)).collect()
}

fn web(root: &Path) -> Result<Vec<Value>, String> {
    xtask::web::check_findings(root)?
        .into_iter()
        .map(|e| {
            let file = format!("web/{}", e.file.trim_start_matches("./"));
            diagnostics::finding_eof(root, &file, e.line, &e.summary, &e.proof, "web")
        })
        .collect()
}

fn head(root: &Path) -> Result<String, String> {
    let mut git = Command::new("git");
    git.current_dir(root).args(["rev-parse", "HEAD"]);
    let out = output(git)?;
    if !out.status.success() {
        return Err(format!("git rev-parse HEAD: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

pub(super) fn run() -> u8 {
    let (base, path) = match arguments() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("xtask checks: {e}");
            return 2;
        }
    };
    let root = xtask::root();
    let mut report = Report { components: Vec::new(), findings: Vec::new(), status: 0 };
    let head = match head(&root) {
        Ok(head) => Some(head),
        Err(e) => {
            eprintln!("xtask checks: {e}");
            report.status = 2;
            None
        }
    };
    let mut test_changes = Vec::new();
    report.step(
        "test changes",
        test_changes::against(&root, &base).map(|flags| {
            for flag in &flags {
                eprintln!("xtask: test change: {flag}");
            }
            test_changes = flags;
            Vec::new()
        }),
    );
    report.step("rustfmt", child("rustfmt"));
    report.step("comments", problems(&root, xtask::comments::check(&root), "comments"));
    report.step("clippy", child("clippy"));
    report.step("audits", problems(&root, super::audits(), "audits"));
    report.step("web", web(&root));
    let document = json!({"status": report.status, "components": report.components,
        "findings": report.findings, "test_changes": test_changes, "base": base, "head": head});
    if let Err(e) = write(&path, &document) {
        eprintln!("xtask checks: couldn't write {}: {e}", path.display());
        return 2;
    }
    eprintln!("xtask: checks: status {}, {} findings", report.status, report.findings.len());
    report.status
}

fn write(path: &Path, document: &Value) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}
