//! `mork test --checks` (fac-165, fac-261): the gate's checks on the clone as it stands, from its
//! branch point, uncommitted changes and new files included; nothing kept.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::slot_client::{Link, fail, io_dir, lease, run_leased};
use crate::runtime::locate::locate;

/// `mork test --checks [paths…]`; its exit code. Paths are accepted and ignored: the gate checks
/// the whole diff from the branch point.
pub(super) fn run(mut link: Link, args: &[String], paths: &[String], own: &str) -> i32 {
    let root = match git(None, &["rev-parse", "--show-toplevel"]) {
        Ok(r) => PathBuf::from(r),
        Err(e) => return fail(&format!("no clone here: {e}"), 2),
    };
    // The checks read the working tree, so they see what isn't committed yet; the gate sees `built`'s head.
    let dirty = match git(Some(&root), &["status", "--porcelain=v1", "--untracked-files=all"]) {
        Ok(s) => !s.is_empty(),
        Err(e) => return fail(&e, 2),
    };
    let answer = match lease(&mut link, args, own, "checks", paths) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let base = answer["base"].as_str().unwrap_or_default();
    let fork = merge_base(&root, base, answer["fork"].as_str());
    let (head, fork) = match (git(Some(&root), &["rev-parse", "HEAD"]), fork) {
        (Ok(h), Ok(f)) => (h, f),
        (Err(e), _) | (_, Err(e)) => return fail(&e, 2),
    };
    let extra = if dirty { ", with the uncommitted changes and new files" } else { "" };
    println!("mork test --checks: the project's checks command on the whole committed diff, {fork}..{head}{extra}");
    if !paths.is_empty() {
        println!("  the paths given are ignored: {}", paths.join(" "));
    }
    let checks = &answer["checks"];
    match checks["command"].as_str() {
        Some(command) => {
            let at = &checks["at"];
            let state = if at["dirty"].as_bool() == Some(true) { "with uncommitted changes" } else { "clean" };
            let commit = at["commit"].as_str().filter(|c| !c.is_empty()).unwrap_or("unknown");
            println!("  checks: {command} --base {fork}, from {}", root.display());
            println!("    the checks folder {} at {commit}, {state}", checks["folder"].as_str().unwrap_or_default());
        }
        None => println!("  checks: none: {}", answer["no_checks"].as_str().unwrap_or("the project has none")),
    }
    match answer["lease"]["id"].as_str() {
        Some(lease) => checks_stage(link, lease, &answer, &root, &fork, own),
        None => 0,
    }
}

// The gate never runs a checks folder with uncommitted changes (4.7.2): neither does this.
fn checks_stage(mut link: Link, lease: &str, answer: &Value, root: &Path, fork: &str, own: &str) -> i32 {
    if answer["checks"]["at"]["dirty"].as_bool() == Some(true) {
        let _ = link.ask(json!({"test.done": {"lease": lease, "exit": 2}}));
        println!("checks: not run: the checks folder has uncommitted changes, and the gate runs none then");
        return 2;
    }
    let io = io_dir();
    let argv: Vec<String> = answer["lease"]["argv"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(|s| s.replace("{io}", &io).replace("{fork}", fork)).collect())
        .unwrap_or_default();
    let json = Path::new(&io).join("checks.json");
    let _ = std::fs::remove_file(&json);
    let code = run_leased(link, lease, &argv, Some(root), own);
    println!("checks: exit {code}");
    let report = std::fs::read_to_string(&json).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    // The seat's folder outlives the turn (fac-654): no later run reads this one's report.
    let _ = std::fs::remove_file(&json);
    let findings = report.as_ref().and_then(|r| r["findings"].as_array()).cloned().unwrap_or_default();
    let located: Vec<_> = findings
        .iter()
        .map(|f| locate(root, f["file"].as_str().unwrap_or_default(), f["line"].as_i64().unwrap_or(0)))
        .collect();
    // The gate's own judgement of the run (4.7.5); the folder's state was judged before it.
    match mork_core::lifecycle::gates::report_verdict(Some(code), report.as_ref(), &located) {
        Err(why) => fail(&format!("checks: the gate wouldn't trust this run: {why}"), 2),
        Ok(0) => 0,
        Ok(n) => {
            println!("checks: {n} finding(s)");
            print_findings(&findings);
            1
        }
    }
}

fn print_findings(findings: &[Value]) {
    for f in findings {
        println!(
            "  {}:{}: {}",
            f["file"].as_str().unwrap_or_default(),
            f["line"],
            f["summary"].as_str().unwrap_or_default()
        );
        if let Some(fix) = f["fix"].as_str() {
            println!("    fix: {fix}");
        }
        for line in f["proof"].as_str().unwrap_or_default().lines() {
            println!("    {line}");
        }
    }
}

// The gate's base: the branch point with the project's base as this clone last fetched it. A clone
// with no `origin/<base>` (a review round's) has the fork Mork gave it.
fn merge_base(root: &Path, base: &str, fork: Option<&str>) -> Result<String, String> {
    if base.is_empty() {
        return Err("Mork named no base branch".into());
    }
    let tracked = format!("refs/remotes/origin/{base}");
    if git(Some(root), &["rev-parse", "--verify", "-q", &tracked]).is_err() {
        return fork
            .map(str::to_owned)
            .ok_or_else(|| format!("no origin/{base} in this clone, and no fork to check from"));
    }
    git(Some(root), &["merge-base", "HEAD", &tracked]).map_err(|e| format!("no branch point with origin/{base}: {e}"))
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let out = cmd.args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
