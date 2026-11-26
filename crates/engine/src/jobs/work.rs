//! `mork work <order.json>`: the git of a clone, run inside its wall (1.2.4, 4.2); the merge train's work in a
//! clone of its own is `runner`'s (fac-68). Each writes what the engine reads into the job's `io/`.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// A bundle and the ref it holds, fetched into `into`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleIn {
    pub bundle: String,
    pub r#ref: String,
    pub into: String,
}

/// Inputs for projecting a preceding review candidate onto the current round's base (fac-628).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebasePrevious {
    pub prev_base: String,
    pub prev_ref: String,
    pub new_base: String,
    pub into: String,
}

/// What a work job does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "work", rename_all = "kebab-case")]
pub enum Order {
    /// Clones a bundle on the branch at `start`, excluding copied project files from commits.
    /// With `base`, integrates its tip first; continued runs start at their own head (fac-111).
    Clone {
        from: BundleIn,
        branch: String,
        start: String,
        exclude: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<BundleIn>,
        /// The base's name, its local branch set to `origin/<base>` (fac-165).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_base: Option<String>,
        /// A review round's previous candidate, kept reachable as `refs/rounds/<n>` (fac-251).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<BundleIn>,
        /// Rare base commits not reachable from `from` or `previous`, fetched then unref'd (fac-628).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extra: Vec<BundleIn>,
        /// Projects `previous` onto the moved base as `refs/rounds/<n>-on-base` (fac-628).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rebase_previous: Option<RebasePrevious>,
    },
    /// The clone's status, its commits past `base_tip` bundled out, and the base in first; with
    /// `fork_in`, a fork point the clone lacked, in before the base (fac-12).
    Bundle {
        base_tip: String,
        base_in: Option<BundleIn>,
        diff: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fork_in: Option<BundleIn>,
        /// The base's name, its local branch moved with `origin/<base>` (fac-165).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_base: Option<String>,
        /// A refresh's: the base in and the status written, no commits bundled out (fac-333).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        base_only: bool,
    },
    /// A merge train's candidate made in an empty folder of its own (fac-68): the head, on the
    /// commit pushed last when there is one, with `on` merged in.
    Candidate {
        head: BundleIn,
        on: BundleIn,
        pushed: Option<BundleIn>,
        branch: String,
        base: String,
        exclude: Vec<String>,
    },
    /// The merge train's run in a candidate's clone (fac-10, fac-68): the install, the checks, the
    /// suite.
    Final {
        expect: String,
        install: Option<String>,
        checks: Option<super::runner::FinalChecks>,
        test: Option<String>,
        /// Mork's own suite's phase in the train, its `MORK_TRAIN_PHASE` (fac-65).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
    },
}

/// What stopped a work order, printed by `mork work` (1.12.4).
#[derive(Debug)]
pub enum WorkError {
    /// git couldn't run.
    Run(std::io::Error),
    /// git refused: the command and its words.
    Git { args: String, stderr: String },
    /// A file of the order or of `io/`: which, and the error.
    Io { what: String, source: std::io::Error },
    /// The order, or a file the work wrote, isn't the JSON it should be.
    Json { what: String, source: serde_json::Error },
    /// The order can't run as it was given.
    Order(String),
}

impl std::fmt::Display for WorkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run(e) => write!(f, "git: {e}"),
            Self::Git { args, stderr } => write!(f, "git {args}: {stderr}"),
            Self::Io { what, source } => write!(f, "{what}: {source}"),
            Self::Json { what, source } => write!(f, "{what}: {source}"),
            Self::Order(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for WorkError {}

pub(super) fn io_err(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> WorkError {
    let what = what.into();
    move |source| WorkError::Io { what, source }
}

pub(super) const NO_HOOKS: [&str; 4] = ["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"];

pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String, WorkError> {
    let out = Command::new("git").args(args).current_dir(dir).output().map_err(WorkError::Run)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
    } else {
        Err(WorkError::Git { args: args.join(" "), stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned() })
    }
}

pub(super) fn git_no_hooks(dir: &Path, args: &[&str]) -> Result<String, WorkError> {
    let mut all = NO_HOOKS.to_vec();
    all.extend_from_slice(args);
    git(dir, &all)
}

pub(super) fn write(io: &Path, name: &str, v: &serde_json::Value) -> Result<(), WorkError> {
    std::fs::write(io.join(name), v.to_string()).map_err(io_err(name))
}

/// Runs the order at `order_path`; the folder it works in is the current one.
pub fn run(order_path: &Path) -> Result<(), WorkError> {
    let text = std::fs::read_to_string(order_path).map_err(io_err(order_path.display().to_string()))?;
    let order: Order =
        serde_json::from_str(&text).map_err(|source| WorkError::Json { what: "the order".into(), source })?;
    let io = order_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let here = std::env::current_dir().map_err(io_err("the working folder"))?;
    match order {
        Order::Clone { from, branch, start, exclude, base, local_base, previous, extra, rebase_previous } => {
            let ins: Vec<&BundleIn> = base.iter().chain(previous.iter()).chain(extra.iter()).chain([&from]).collect();
            let at = Start { branch: &branch, start: &start, local_base: local_base.as_deref() };
            clone(&here, &io, &ins, &at, &exclude, &extra, rebase_previous.as_ref())
        }
        Order::Bundle { base_tip, base_in, diff, fork_in, local_base, base_only } => {
            if let Some(f) = &fork_in {
                fetch_in(&here, f)?;
            }
            if let Some(b) = &base_in {
                fetch_forward(&here, b)?;
            }
            bundle(&here, &io, &base_tip, diff, local_base.as_deref(), !base_only)
        }
        Order::Candidate { head, on, pushed, branch, base, exclude } => {
            let pushed = pushed.as_ref();
            let o = super::runner::CandidateOrder {
                head: &head,
                on: &on,
                pushed,
                branch: &branch,
                base: &base,
                exclude: &exclude,
            };
            super::runner::candidate(&here, &io, &o)
        }
        Order::Final { expect, install, checks, test, phase } => {
            let run = super::runner::RunOrder {
                install: install.as_deref(),
                checks: checks.as_ref(),
                test: test.as_deref(),
                phase: phase.as_deref(),
            };
            super::runner::final_run(&here, &io, &expect, &run)
        }
    }
}

pub(super) fn fetch_in(dir: &Path, b: &BundleIn) -> Result<(), WorkError> {
    let spec = format!("+{}:{}", b.r#ref, b.into);
    git(dir, &["fetch", "-q", "--no-tags", &b.bundle, &spec]).map(|_| ())
}

// Where a base's tip lands before origin/<base> moves to it.
const BASE_IN: &str = "refs/mork/base-in";

// origin/<base> moves to the tip brought in, never back from a newer one a refresh brought (fac-333).
fn fetch_forward(dir: &Path, b: &BundleIn) -> Result<(), WorkError> {
    fetch_in(dir, &BundleIn { into: BASE_IN.into(), ..b.clone() })?;
    if git(dir, &["merge-base", "--is-ancestor", BASE_IN, &b.into]).is_err() {
        git(dir, &["update-ref", &b.into, BASE_IN])?;
    }
    git(dir, &["update-ref", "-d", BASE_IN]).map(|_| ())
}

struct Start<'a> {
    branch: &'a str,
    start: &'a str,
    local_base: Option<&'a str>,
}

fn clone(
    dir: &Path,
    io: &Path,
    ins: &[&BundleIn],
    at: &Start<'_>,
    exclude: &[String],
    extra: &[BundleIn],
    rebase: Option<&RebasePrevious>,
) -> Result<(), WorkError> {
    let (branch, start) = (at.branch, at.start);
    git(dir, &["init", "-q"])?;
    if !exclude.is_empty() {
        let lines: String = exclude.iter().map(|f| format!("/{}\n", f.trim_start_matches('/'))).collect();
        std::fs::write(dir.join(".git/info/exclude"), lines).map_err(io_err("info/exclude"))?;
    }
    for b in ins {
        fetch_in(dir, b)?;
    }
    git(dir, &["checkout", "-q", "-b", branch, start])?;
    local_base(dir, at.local_base, branch)?;
    if let Some(rp) = rebase {
        project_previous(dir, io, rp)?;
    }
    // Keep fetched base objects resolvable by SHA without leaving temporary refs behind (fac-628).
    for b in extra {
        let _ = git_no_hooks(dir, &["update-ref", "-d", &b.into]);
    }
    status(dir, io, start, false).map(|_| ())
}

// Projecting onto the moved base runs inside the clone's wall, never in the shared mirror (fac-628).
fn project_previous(dir: &Path, io: &Path, rp: &RebasePrevious) -> Result<(), WorkError> {
    let mb = format!("--merge-base={}", rp.prev_base);
    let out = Command::new("git")
        .args(NO_HOOKS)
        .args(["merge-tree", "--write-tree", "--name-only", "--no-messages", "-z", &mb, &rp.prev_ref, &rp.new_base])
        .current_dir(dir)
        .output();
    let Ok(out) = out else { return Ok(()) };
    let conflicted = match out.status.code() {
        Some(0) => false,
        Some(1) => true,
        _ => return Ok(()),
    };
    let Some((tree, conflicts)) = parse_merge_tree(&out.stdout, conflicted) else { return Ok(()) };
    let commit_args = [
        "-c",
        "user.name=review",
        "-c",
        "user.email=review@local",
        "commit-tree",
        &tree,
        "-p",
        &rp.prev_ref,
        "-p",
        &rp.new_base,
        "-m",
        "Previous round on current base",
    ];
    let Ok(commit) = git_no_hooks(dir, &commit_args) else { return Ok(()) };
    if git_no_hooks(dir, &["update-ref", &rp.into, &commit]).is_err() {
        return Ok(());
    }
    write(io, "rebased.json", &json!({"status": "projected", "conflicted": conflicted, "conflicts": conflicts}))
}

fn parse_merge_tree(stdout: &[u8], conflicted: bool) -> Option<(String, Vec<String>)> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut parts = text.split('\0');
    let tree = parts.next()?.trim();
    if tree.len() < 40 || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut conflicts = Vec::new();
    if conflicted {
        for path in parts.take_while(|p| !p.is_empty()) {
            conflicts.push(path.to_owned());
        }
        conflicts.sort();
        conflicts.dedup();
    }
    Some((tree.to_owned(), conflicts))
}

fn status(dir: &Path, io: &Path, base_tip: &str, diff: bool) -> Result<(String, Option<String>, usize), WorkError> {
    let branch = git(dir, &["symbolic-ref", "-q", "--short", "HEAD"]).unwrap_or_default();
    let head = git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).ok().filter(|h| h.len() == 40);
    let porcelain = git(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let dirty = dirty_paths(&porcelain);
    let ahead = match &head {
        Some(h) if h != base_tip => {
            let range = format!("{base_tip}..{h}");
            git(dir, &["rev-list", "--count", &range]).ok().and_then(|n| n.trim().parse().ok()).unwrap_or(1)
        }
        _ => 0,
    };
    if diff {
        let d = git(dir, &["diff", "--no-color", base_tip]).unwrap_or_default();
        std::fs::write(io.join("diff"), d).map_err(io_err("diff"))?;
    }
    let st = json!({"branch": branch, "head": head, "clean": dirty.is_empty(), "dirty": dirty, "bundle": false});
    write(io, "status.json", &st)?;
    Ok((branch, head, ahead))
}

// The local base on `origin/<base>`, for `git log <base>..HEAD` to read as the gate does (fac-165):
// never the branch checked out, never in a clone with no `origin/<base>`.
fn local_base(dir: &Path, base: Option<&str>, branch: &str) -> Result<(), WorkError> {
    let Some(base) = base.filter(|b| *b != branch) else { return Ok(()) };
    let tracked = format!("refs/remotes/origin/{base}");
    if git(dir, &["rev-parse", "--verify", "-q", &tracked]).is_err() {
        return Ok(());
    }
    git(dir, &["update-ref", &format!("refs/heads/{base}"), &tracked]).map(|_| ())
}

pub(super) fn dirty_paths(z: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut parts = z.split('\0').filter(|p| !p.is_empty());
    while let Some(entry) = parts.next() {
        let (xy, path) = (entry.get(..2).unwrap_or(""), entry.get(3..).unwrap_or(""));
        out.push(path.to_owned());
        if (xy.starts_with('R') || xy.starts_with('C'))
            && let Some(src) = parts.next()
        {
            out.push(src.to_owned());
        }
    }
    out
}

fn bundle(dir: &Path, io: &Path, base_tip: &str, diff: bool, base: Option<&str>, out: bool) -> Result<(), WorkError> {
    let (branch, head, ahead) = status(dir, io, base_tip, diff)?;
    local_base(dir, base, &branch)?;
    let Some(head) = head.filter(|_| out && ahead > 0) else { return Ok(()) };
    let out = io.join("out.bundle").display().to_string();
    let tip = if branch.is_empty() { "HEAD".to_owned() } else { branch };
    let not = format!("^{base_tip}");
    git(dir, &["bundle", "create", "-q", &out, &tip, &not])?;
    let mut st: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(io.join("status.json")).map_err(io_err("status.json"))?)
            .map_err(|source| WorkError::Json { what: "status.json".into(), source })?;
    st["bundle"] = json!(true);
    st["head"] = json!(head);
    write(io, "status.json", &st)
}

pub(super) fn sh(dir: &Path, cmd: &str, args: &[&str]) -> (i32, String) {
    let script = format!("{cmd} \"$@\"");
    let mut argv = vec!["-c", script.as_str(), "sh"];
    argv.extend_from_slice(args);
    match Command::new("sh").args(&argv).current_dir(dir).output() {
        Ok(o) => {
            let mut words = String::from_utf8_lossy(&o.stdout).into_owned();
            words.push_str(&String::from_utf8_lossy(&o.stderr));
            let tail: Vec<&str> = words.lines().rev().take(20).collect();
            (o.status.code().unwrap_or(-1), tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
        }
        Err(e) => (-1, e.to_string()),
    }
}

#[cfg(test)]
// why: test code: panicking on setup or assertion failure fails the test.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn commit(dir: &Path, files: &[(&str, &str)], msg: &str) -> String {
        for (p, body) in files {
            std::fs::write(dir.join(p), body).unwrap();
        }
        git(dir, &["add", "-A"]).unwrap();
        git(dir, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-qm", msg]).unwrap();
        git(dir, &["rev-parse", "HEAD"]).unwrap()
    }

    fn bundle_ref(repo: &Path, path: &Path, r#ref: &str, commit: &str, into: &str) -> BundleIn {
        git(repo, &["update-ref", r#ref, commit]).unwrap();
        git(repo, &["bundle", "create", "-q", &path.display().to_string(), r#ref]).unwrap();
        BundleIn { bundle: path.display().to_string(), r#ref: r#ref.into(), into: into.into() }
    }

    #[test]
    fn parse_merge_tree_handles_clean_conflicted_and_invalid_output() {
        let oid = "a".repeat(40);
        assert_eq!(parse_merge_tree(format!("{oid}\0").as_bytes(), false), Some((oid.clone(), Vec::new())));
        let raw = format!("{oid}\0b.rs\0a.rs\0a.rs\0\0CONFLICT\0msg\0");
        assert_eq!(parse_merge_tree(raw.as_bytes(), true), Some((oid, vec!["a.rs".into(), "b.rs".into()])));
        assert_eq!(parse_merge_tree(b"not-an-oid\0", false), None);
    }

    fn run_rebased_clone(
        root: &Path,
        src: &Path,
        name: &str,
        bases: (&str, &str),
        commits: (&str, &str),
    ) -> (serde_json::Value, String, std::path::PathBuf) {
        let (io, dst) = (root.join(format!("io-{name}")), root.join(format!("dst-{name}")));
        std::fs::create_dir_all(&io).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let ins = [
            bundle_ref(src, &io.join("prev.bundle"), "refs/mork/prev", commits.0, "refs/rounds/1"),
            bundle_ref(src, &io.join("curr.bundle"), "refs/mork/curr", commits.1, "refs/remotes/origin/card"),
        ];
        let refs: Vec<&BundleIn> = ins.iter().collect();
        let at = Start { branch: "card", start: "refs/remotes/origin/card", local_base: None };
        let rp = RebasePrevious {
            prev_base: bases.0.into(),
            prev_ref: "refs/rounds/1".into(),
            new_base: bases.1.into(),
            into: "refs/rounds/1-on-base".into(),
        };
        clone(&dst, &io, &refs, &at, &[], &[], Some(&rp)).unwrap();
        let rebased = serde_json::from_str(&std::fs::read_to_string(io.join("rebased.json")).unwrap()).unwrap();
        let diff = git(&dst, &["diff", "refs/rounds/1-on-base", "HEAD"]).unwrap();
        (rebased, diff, dst)
    }

    #[test]
    fn clone_fetches_extra_base_objects_and_clears_temp_refs() {
        let root = std::env::temp_dir().join(format!("fac628-extra-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, io, dst) = (root.join("src"), root.join("io"), root.join("dst"));
        for d in [&src, &io, &dst] {
            std::fs::create_dir_all(d).unwrap();
        }
        git(&src, &["init", "-qb", "master"]).unwrap();
        let b1 = commit(&src, &[("a.txt", "1\n")], "b1");
        let c2 = commit(&src, &[("a.txt", "2\n")], "c2");
        let b2 = commit(&src, &[("a.txt", "2\n"), ("base_only.txt", "x\n")], "b2");
        let from = bundle_ref(&src, &io.join("curr.bundle"), "refs/mork/curr", &c2, "refs/remotes/origin/card");
        let extra = vec![bundle_ref(&src, &io.join("base.bundle"), "refs/mork/new-base", &b2, "refs/mork/new-base")];
        let ins: Vec<&BundleIn> = extra.iter().chain([&from]).collect();
        let at = Start { branch: "card", start: "refs/remotes/origin/card", local_base: None };
        clone(&dst, &io, &ins, &at, &[], &extra, None).unwrap();
        assert_eq!(git(&dst, &["for-each-ref", "refs/mork/"]).unwrap(), "");
        let base_diff = git(&dst, &["diff", "--name-only", &b1, &b2]).unwrap();
        assert!(base_diff.contains("base_only.txt"), "missing base diff: {base_diff}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clone_projects_previous_round_onto_moved_base_clean_and_conflicted() {
        let root = std::env::temp_dir().join(format!("fac628-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        git(&src, &["init", "-qb", "master"]).unwrap();
        let b1 = commit(&src, &[("shared.txt", "top\n\nmid\n\nbot\n"), ("other.txt", "o1\n")], "b1");
        git(&src, &["checkout", "-qb", "card"]).unwrap();
        let c1 = commit(&src, &[("shared.txt", "card-top\n\nmid\n\nbot\n")], "r1");
        git(&src, &["checkout", "-q", "master"]).unwrap();
        let b2 = commit(&src, &[("shared.txt", "top\n\nmid\n\nmaster-bot\n"), ("other.txt", "o2\n")], "b2");
        git(&src, &["checkout", "-q", "card"]).unwrap();
        git(&src, &["-c", "user.name=T", "-c", "user.email=t@e", "merge", "-qm", "merge b2", &b2]).unwrap();
        let c2 = commit(&src, &[("shared.txt", "card-top-fixed\n\nmid\n\nmaster-bot\n")], "r2");

        let (rebased, diff, dst) = run_rebased_clone(&root, &src, "clean", (&b1, &b2), (&c1, &c2));
        assert_eq!(rebased, json!({"status": "projected", "conflicted": false, "conflicts": []}));
        assert_eq!(git(&dst, &["rev-parse", "refs/rounds/1"]).unwrap(), c1);
        assert_eq!(git(&dst, &["for-each-ref", "refs/mork/"]).unwrap(), "");
        assert!(diff.contains("-card-top") && diff.contains("+card-top-fixed"), "missing card fix: {diff}");
        assert!(!diff.contains("other.txt") && !diff.contains("master-bot"), "leaked base changes: {diff}");

        git(&src, &["checkout", "-q", "master"]).unwrap();
        let b3 = commit(&src, &[("shared.txt", "master-top\n\nmid\n\nmaster-bot\n"), ("other.txt", "o3\n")], "b3");
        git(&src, &["checkout", "-q", "card"]).unwrap();
        let _ = git(&src, &["merge", &b3]);
        let c3 = commit(&src, &[("shared.txt", "resolved-top\n\nmid\n\nmaster-bot\n")], "merge b3");
        let (rebased, diff, _) = run_rebased_clone(&root, &src, "conflict", (&b2, &b3), (&c2, &c3));
        assert_eq!(rebased, json!({"status": "projected", "conflicted": true, "conflicts": ["shared.txt"]}));
        assert!(diff.contains("+resolved-top") && !diff.contains("other.txt"), "bad conflict diff: {diff}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
