// The merge train's jobs (fac-68): a candidate made in its own disposable clone `trains/<card>-<tag>` from the mirror's
// commits, its checks credited to the checks folder's commit as the gate's are. The card's clone is never read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mork_core::decide::JobEnd;
use mork_types::config::ProjectConfig as Project;
use mork_types::job::JobKind;
use mork_types::work::{CheckRun, Checkout, FinalRun, Purpose};
use mork_types::{CardId, Sha};
use serde_json::Value;

use super::Engine;
use super::work::{Plan, finished, json_of, plan};
use crate::jobs::runner::FinalChecks;
use crate::jobs::work::Order;

// A train's run of `project` in its candidate's clone `dir`. Mork's `own` suite runs in the
// train's two phases: the second outside every wall, without the install the first ran (fac-65).
fn train_plan(
    purpose: &Purpose,
    project: (&str, &Project),
    own: bool,
    argv: Vec<String>,
    dir: String,
) -> Result<Plan, String> {
    let (name, p) = project;
    let Purpose::TrainRun { card: _, commit, on, clone: _, checks, test, unwalled } = purpose else {
        return Err("not a train's run".into());
    };
    let k = p.checks.clone().filter(|_| *checks);
    let order = Order::Final {
        expect: commit.0.clone(),
        install: p.install.clone().filter(|_| !unwalled),
        checks: k.as_ref().map(|k| FinalChecks { command: k.command.clone(), base: on.0.clone() }),
        // The command the job was made with, whatever a reload set since (M4.11).
        test: test.clone(),
        phase: own.then(|| if *unwalled { "unwalled" } else { "walled" }.to_owned()),
    };
    let mut pl = plan(purpose, JobKind::Check, argv, &dir, name).rw(dir);
    pl.unwalled = *unwalled;
    if *unwalled {
        pl.files.insert("tmp/.keep".into(), String::new());
    }
    if let Some(k) = k {
        pl = pl.ro(k.folder);
    }
    pl.files.insert("work.json".into(), json_of(&order)?);
    Ok(pl)
}

pub(super) fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

fn run_result(commit: &Sha, j: &Value, checks: Option<CheckRun>) -> Result<FinalRun, String> {
    let checkout = |k: &str| serde_json::from_value::<Checkout>(j[k].clone()).map_err(|e| format!("{k}: {e}"));
    Ok(FinalRun {
        head: commit.clone(),
        before: checkout("before")?,
        after: checkout("after")?,
        checks,
        ran: j["ran"].as_bool() == Some(true),
        exit: j["exit"].as_i64().and_then(|c| i32::try_from(c).ok()),
        output: j["output"].as_str().unwrap_or("").to_owned(),
        phase: serde_json::from_value(j["phase"].clone()).map_err(|e| format!("phase: {e}"))?,
        failing: None,
        logs: BTreeMap::new(),
    })
}

// Each Rust package's folder in the candidate's clone, by its name, from its workspace's members:
// xtask names a binary's source from its package's folder (fac-164).
fn packages(dir: &Path) -> BTreeMap<String, String> {
    let name = |folder: &str| -> Option<String> {
        let text = std::fs::read_to_string(dir.join(folder).join("Cargo.toml")).ok()?;
        let t: toml::Table = text.parse().ok()?;
        Some(t.get("package")?.get("name")?.as_str()?.to_owned())
    };
    let root = std::fs::read_to_string(dir.join("Cargo.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok());
    let members = root.as_ref().and_then(|t| t.get("workspace")?.get("members")?.as_array().cloned());
    let folders = members.into_iter().flatten().filter_map(|m| m.as_str().map(str::to_owned));
    let mut out: BTreeMap<String, String> = folders.filter_map(|f| Some((name(&f)?, f))).collect();
    if let Some(n) = name("") {
        out.insert(n, String::new());
    }
    out
}

impl Engine {
    /// A candidate's clone: one per candidate, so none is ever reused under a job still ending.
    pub(super) fn train_dir(&self, card: &CardId, clone: i64) -> PathBuf {
        self.opts.home.trains().join(format!("{card}-{clone}"))
    }

    // The candidate's clone read-write, the checks folder read-only (M3.28). Mork's own suite
    // is told its phase; its second, outside every wall, finds the install done by the first (fac-65).
    pub(super) fn plan_train_run(&self, purpose: &Purpose) -> Result<Plan, String> {
        let Purpose::TrainRun { card, clone, .. } = purpose else {
            return Err("not a train's run".into());
        };
        let c = self.card(card)?;
        let mut p = self.project(&c.project)?;
        // The folder the run mounts: the checks' checkout (fac-165).
        if let Some(k) = p.checks.as_mut() {
            k.folder = self.checks_mount(&k.folder);
        }
        let dir = self.train_dir(card, *clone).display().to_string();
        let own = self.ctx.config.mork.project.as_ref() == Some(&c.project);
        train_plan(purpose, (c.project.as_str(), &p), own, self.work_argv(), dir)
    }

    pub(super) fn train_result(
        &self,
        job: &str,
        purpose: &Purpose,
        io: &Path,
        started: Option<&str>,
    ) -> Result<JobEnd, String> {
        let Purpose::TrainRun { card, commit, clone, .. } = purpose else { return Err("not a train's run".into()) };
        let j = read_json(&io.join("final.json")).ok_or("it wrote no final.json")?;
        let dir = self.train_dir(card, *clone);
        let checks = j["checks_exit"].as_i64().and_then(|c| i32::try_from(c).ok()).map(|code| {
            let report = read_json(&io.join("final-checks.json"));
            self.final_checks(card, commit, &dir, code, report, started)
        });
        let mut run = run_result(commit, &j, checks)?;
        // Its failures lead, read from the whole output the tail cut them from (fac-144).
        if run.ran && run.exit != Some(0) {
            let log = io.join("suite.out");
            let whole = std::fs::read_to_string(&log).unwrap_or_default();
            run.failing = mork_core::queue::failing::read(&whole, &packages(&dir));
            let failing = run.failing.as_deref();
            run.output = mork_core::queue::suite_log::told(&whole, &run.output, &log.display().to_string(), failing);
            // Its log's copies (fac-164).
            let c = self.card(card)?;
            run.logs = self.suite_logs(&c, &log, job);
        }
        Ok(finished(serde_json::to_value(run).map_err(|e| e.to_string())?))
    }

    // Judged by the gate's rules (4.7.5): their exit, their report, its findings located in the
    // candidate's clone, the folder's commit read at the launch and refused when it moved since (M3.12).
    pub(super) fn final_checks(
        &self,
        card: &CardId,
        head: &Sha,
        clone: &Path,
        code: i32,
        report: Option<Value>,
        started: Option<&str>,
    ) -> CheckRun {
        let located = report
            .as_ref()
            .and_then(|r| r.get("findings").and_then(Value::as_array))
            .map(|fs| {
                let at = |f: &Value| {
                    let file = f.get("file").and_then(Value::as_str).unwrap_or("");
                    super::locate::locate(clone, file, f.get("line").and_then(Value::as_i64).unwrap_or(0))
                };
                fs.iter().map(at).collect()
            })
            .unwrap_or_default();
        let (now, folder_dirty) = self.checks_folder(card);
        let folder_commit = started.map_or_else(|| now.clone(), str::to_owned);
        CheckRun {
            head: head.clone(),
            exit: Some(code),
            report,
            folder_moved: folder_commit != now,
            folder_commit,
            folder_dirty,
            located,
        }
    }

    // A run whose checks folder has uncommitted changes never runs (4.7.2): its checks are
    // untrusted at once, and the train holds.
    pub(super) fn dirty_final(&self, card: &CardId, head: &Sha) -> Option<JobEnd> {
        let (folder_commit, dirty) = self.checks_folder(card);
        if !dirty {
            return None;
        }
        let none = Checkout { head: None, tree: None, tracked_clean: false, clean: false };
        let checks = CheckRun {
            head: head.clone(),
            exit: None,
            report: None,
            folder_commit,
            folder_dirty: true,
            located: Vec::new(),
            folder_moved: false,
        };
        let run = FinalRun {
            head: head.clone(),
            before: none.clone(),
            after: none,
            checks: Some(checks),
            ran: false,
            exit: None,
            output: String::new(),
            phase: None,
            failing: None,
            logs: BTreeMap::new(),
        };
        Some(finished(serde_json::to_value(run).unwrap_or_default()))
    }
}
