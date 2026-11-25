//! The merge train's work in a disposable clone of a card's own (fac-68), inside its wall: the candidate is made
//! from the mirror's commits, never touching the card's branch; each run writes what the engine reads into `io/`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::work::{BundleIn, WorkError, dirty_paths, io_err, sh, write};

/// A final run's checks: their command, and the base they run against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalChecks {
    pub command: String,
    pub base: String,
}

/// What a candidate is made of: the card's head, the commit it pushed last when it has one, the
/// commit it is made on, its branch and the base's name, and the copy files kept out of git.
pub struct CandidateOrder<'a> {
    pub head: &'a BundleIn,
    pub on: &'a BundleIn,
    pub pushed: Option<&'a BundleIn>,
    pub branch: &'a str,
    pub base: &'a str,
    pub exclude: &'a [String],
}

// The queue's git runs no hook and no fsmonitor: its merges are no agent's, and its reads of the
// checkout are the engine's own, not what a planted command says.
fn git(dir: &Path, args: &[&str]) -> Result<String, WorkError> {
    super::work::git_no_hooks(dir, args)
}

fn fetch_in(dir: &Path, b: &BundleIn) -> Result<(), WorkError> {
    let spec = format!("+{}:{}", b.r#ref, b.into);
    git(dir, &["fetch", "-q", "--no-tags", &b.bundle, &spec]).map(|_| ())
}

fn head(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).ok().filter(|h| h.len() == 40)
}

fn porcelain(dir: &Path, untracked: &str) -> Result<Vec<String>, WorkError> {
    git(dir, &["status", "--porcelain=v1", "-z", untracked]).map(|z| dirty_paths(&z))
}

// `what` merged in with `git merge --no-ff`, as a join is, unless one holds the other (a
// base past the head: its move refuses it, 4.11.3); a conflict's paths, its merge aborted.
fn merge_in(dir: &Path, what: &str, message: &str) -> Result<Vec<String>, WorkError> {
    let ancestor = |a: &str, b: &str| git(dir, &["merge-base", "--is-ancestor", a, b]).is_ok();
    if ancestor(what, "HEAD") || ancestor("HEAD", what) {
        return Ok(Vec::new());
    }
    if git(dir, &["merge", "-q", "--no-ff", "-m", message, what]).is_ok() {
        return Ok(Vec::new());
    }
    let list = git(dir, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
    git(dir, &["merge", "--abort"])?;
    Ok(list.lines().map(str::to_owned).collect())
}

/// The candidate, in an empty folder: the branch at the commit pushed last, or at the head; the head merged in,
/// then the commit made. A conflict is named, its merge aborted; the commit is bundled out for the mirror.
pub fn candidate(dir: &Path, io: &Path, o: &CandidateOrder<'_>) -> Result<(), WorkError> {
    git(dir, &["init", "-q"])?;
    if !o.exclude.is_empty() {
        let lines: String = o.exclude.iter().map(|f| format!("/{}\n", f.trim_start_matches('/'))).collect();
        std::fs::write(dir.join(".git/info/exclude"), lines).map_err(io_err("info/exclude"))?;
    }
    for b in [Some(o.head), Some(o.on), o.pushed].into_iter().flatten() {
        fetch_in(dir, b)?;
    }
    // On the commit pushed last unless the head holds it: a push is never forced (4.10.1).
    let ancestor = |a: &str, b: &str| git(dir, &["merge-base", "--is-ancestor", a, b]).is_ok();
    let pushed = o.pushed.filter(|b| !ancestor(&b.into, &o.head.into));
    git(dir, &["checkout", "-q", "-b", o.branch, &pushed.unwrap_or(o.head).into])?;
    let mut conflicts = Vec::new();
    if pushed.is_some() {
        conflicts = merge_in(dir, &o.head.into, &format!("Merge the head of {} into it", o.branch))?;
    }
    let into_pushed = !conflicts.is_empty();
    if conflicts.is_empty() {
        conflicts = merge_in(dir, &o.on.into, &format!("Merge {} into {}", o.base, o.branch))?;
    }
    if !conflicts.is_empty() {
        let made = json!({"conflicts": conflicts, "into_pushed": into_pushed, "commit": null, "tree": null});
        return write(io, "made.json", &made);
    }
    let made = head(dir);
    let tree = git(dir, &["rev-parse", "HEAD^{tree}"]).ok();
    let mut not = vec![format!("^{}", o.head.into), format!("^{}", o.on.into)];
    not.extend(o.pushed.map(|b| format!("^{}", b.into)));
    let mut count = vec!["rev-list", "--count", "HEAD"];
    count.extend(not.iter().map(String::as_str));
    let new = git(dir, &count).is_ok_and(|n| n.trim() != "0");
    if new {
        let out = io.join("out.bundle").display().to_string();
        let mut args = vec!["bundle", "create", "-q", out.as_str(), o.branch];
        args.extend(not.iter().map(String::as_str));
        git(dir, &args)?;
    }
    write(io, "made.json", &json!({"conflicts": [], "commit": made, "tree": tree}))
}

fn checkout(dir: &Path) -> Result<Value, WorkError> {
    let tracked = porcelain(dir, "--untracked-files=no")?.is_empty();
    let all = porcelain(dir, "--untracked-files=all")?.is_empty();
    let tree = git(dir, &["rev-parse", "HEAD^{tree}"]).ok();
    Ok(json!({"head": head(dir), "tree": tree, "tracked_clean": tracked, "clean": all}))
}

// Its whole output goes to `io/suite.out`; the result keeps its tail, as a review card's suite's.
fn suite(dir: &Path, io: &Path, test: &str, phase: Option<&str>) -> Result<(Option<i32>, String), WorkError> {
    let path = io.join("suite.out");
    // Made anew, never the install's truncated: the engine reads the suite's as it grows (fac-697).
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_err("suite.out")(e)),
        _ => {}
    }
    let out = std::fs::File::create(&path).map_err(io_err("suite.out"))?;
    let err = out.try_clone().map_err(io_err("suite.out"))?;
    let mut sh = Command::new("sh");
    if let Some(phase) = phase {
        sh.env("FACTORY_TRAIN_PHASE", phase);
    }
    let tmp = (phase == Some("unwalled"))
        .then(|| super::folder::ShortTemp::new(&std::env::temp_dir()))
        .transpose()
        .map_err(io_err("the host suite's temporary alias"))?;
    if let Some(tmp) = &tmp {
        sh.env("TMPDIR", tmp.path());
    }
    let status = sh
        .args(["-c", test])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .status()
        .map_err(io_err("the suite"))?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let tail: Vec<&str> = text.lines().rev().take(60).collect();
    Ok((status.code(), tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
}

/// What a run runs: the project's install, its checks, and its suite, told its phase in the train.
#[derive(Clone, Copy)]
pub struct RunOrder<'a> {
    pub install: Option<&'a str>,
    pub checks: Option<&'a FinalChecks>,
    pub test: Option<&'a str>,
    pub phase: Option<&'a str>,
}

/// The run: the checkout read, the install, the checks and the suite when it is the expected
/// candidate and clean, the checkout read again. `core` judges whether it changed.
pub fn final_run(dir: &Path, io: &Path, expect: &str, run: &RunOrder<'_>) -> Result<(), WorkError> {
    let RunOrder { install, checks, test, phase } = *run;
    let before = checkout(dir)?;
    let ready = before["head"].as_str() == Some(expect) && before["clean"].as_bool() == Some(true);
    let (mut checks_exit, mut ran, mut exit, mut output) = (None, false, None, String::new());
    // The project's install first, in a clone just made: a failure is the run's, its exit read as
    // the suite's would be.
    if let Some(i) = install.filter(|_| ready) {
        let (code, words) = suite(dir, io, i, None)?;
        if code != Some(0) {
            let j = json!({"before": before, "after": checkout(dir)?, "checks_exit": null, "ran": true,
                           "exit": code, "output": format!("The install failed.\n{words}")});
            return write(io, "final.json", &j);
        }
    }
    if ready {
        if let Some(k) = checks {
            let json_out = io.join("final-checks.json").display().to_string();
            checks_exit = Some(sh(dir, &k.command, &["--base", &k.base, "--json", &json_out]).0);
        }
        if let Some(t) = test {
            (exit, output) = suite(dir, io, t, phase)?;
            ran = true;
        }
    }
    let after = checkout(dir)?;
    let j = json!({"before": before, "after": after, "checks_exit": checks_exit, "ran": ran, "exit": exit,
                   "output": output, "phase": phase.filter(|_| ran)});
    write(io, "final.json", &j)
}
