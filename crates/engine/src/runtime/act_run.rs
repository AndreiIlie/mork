// One outward action (2.5), off the engine's thread: what it needs is copied into `Call`, so its result
// or failure (2.5.7) is all that comes back.

use std::collections::BTreeMap;
use std::path::Path;

use mork_types::action::{Action, ActionWhat};
use mork_types::card::{Card, Mode, Stage};
use mork_types::config::{Config, ProjectConfig as Project};
use mork_types::work::{ActionFailure, Taken};
use mork_types::{CardId, Sha};
use serde_json::{Value, json};

use crate::git::{self, checkout, mirror};
use crate::paths::Home;
use crate::secrets::Secrets;

/// What every action may need, copied from the engine.
#[derive(Debug, Clone)]
pub struct Call {
    pub home: Home,
    pub config: Config,
    pub telemetry: BTreeMap<String, String>,
    pub action: Action,
    /// The action's card, as the state has it.
    pub card: Option<Card>,
    /// Each live card of the action's project with a head (a fetch says which hold the tip).
    pub heads: Vec<(CardId, Sha)>,
    /// The branches whose tips a fetch reports: a review card's, and its PR's destination (fac-95).
    pub branches: Vec<String>,
    /// Completed same-body comments on this PR, with IDs when available, excluded from crash
    /// recovery so another finding's thread cannot be claimed (R18, fac-111).
    pub same_done: Vec<Option<u64>>,
    pub now: i64,
    /// The token's verdict (3.7.2), read under the project's lock right before the call (M4.5).
    pub verdict: super::verdict::Verdict,
    /// Bitbucket's short hashes made full, the pipe's (fac-361).
    pub(super) readers: crate::forge::Readers,
    /// Where a token file changed since Bitbucket was asked about it sends what it now says.
    pub tx: std::sync::mpsc::Sender<super::Input>,
    pub project: mork_types::ProjectId,
    /// The running binary: the door's dispatcher, the engine's own local child for every target (fac-1).
    pub bin: std::path::PathBuf,
    /// Set once a door read's last waiting caller has gone: the read is cut (fac-183).
    pub stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub fn failed(words: impl Into<String>) -> ActionFailure {
    ActionFailure::Failed { words: words.into() }
}

/// A Git authentication refusal is a token failure; other errors keep Git's words.
pub fn git_failure(words: impl Into<String>) -> ActionFailure {
    let words = words.into();
    if words.contains("401") || words.contains("Authentication failed") {
        ActionFailure::Token { words }
    } else {
        failed(words)
    }
}

pub(super) use crate::forge::source;

impl Call {
    pub fn project(&self, name: &str) -> Result<Project, ActionFailure> {
        self.config.projects.get(name).cloned().ok_or_else(|| failed(format!("no project `{name}`")))
    }

    pub fn card(&self) -> Result<&Card, ActionFailure> {
        self.card.as_ref().ok_or_else(|| failed("the action's card is gone"))
    }

    pub fn mirror(&self, project: &str) -> Result<std::path::PathBuf, ActionFailure> {
        mirror::ensure(&self.home, project).map_err(failed)
    }

    pub fn secrets(&self) -> Secrets {
        Secrets::gather(&self.home, &self.config, &self.telemetry)
    }

    pub fn door_redactor(&self) -> crate::door::redact::Redactor {
        crate::door::redactor(&self.home, &self.secrets())
    }
}

/// Whether an action reaches Bitbucket: its calls, and a fetch of a project on it.
pub fn reaches_forge(config: &Config, what: &ActionWhat) -> bool {
    match what {
        ActionWhat::Push { .. }
        | ActionWhat::OpenPr { .. }
        | ActionWhat::Comment { .. }
        | ActionWhat::ReplyComment { .. }
        | ActionWhat::ResolveComment { .. }
        | ActionWhat::Merge { .. } => true,
        ActionWhat::Fetch { project, .. } => config.projects.get(project).is_some_and(|p| p.forge.hosted()),
        ActionWhat::Take { .. }
        | ActionWhat::TakeJoin { .. }
        | ActionWhat::BackupPush { .. }
        | ActionWhat::MoveBase { .. }
        | ActionWhat::CreateBoardCard { .. }
        | ActionWhat::ChangeBoardCard { .. }
        | ActionWhat::RemoveClone { .. }
        | ActionWhat::RemoveTrain { .. }
        | ActionWhat::Door(_) => false,
    }
}

// The guards needing no read of their own (2.5.8): no Bitbucket call under a token found the operator's (3.7.2), its verdict
// read now under the lock, re-asked when the token file changed (M4.5); the call uses only the text read here (M5.1).
fn guard(c: &Call) -> Result<Option<crate::forge::Credential>, ActionFailure> {
    if !reaches_forge(&c.config, &c.action.what) {
        return Ok(None);
    }
    let forge = c.project(c.project.as_str())?.forge;
    let credential = crate::forge::Credential::read(&forge)?;
    let reader = credential.pinned(|| c.readers.read(&forge))?;
    match c.verdict.now(reader.as_ref(), credential.text(), forge.label(), &c.project, &c.tx) {
        Ok(None) => Ok(Some(credential)),
        Ok(Some(words)) | Err(words) => Err(ActionFailure::Token { words }),
    }
}

pub fn run(c: &Call) -> Result<Value, ActionFailure> {
    match guard(c)? {
        Some(credential) => credential.pinned(|| call(c)),
        None => call(c),
    }
}

fn call(c: &Call) -> Result<Value, ActionFailure> {
    match &c.action.what {
        ActionWhat::Fetch { project, .. } => fetch(c, project),
        ActionWhat::Take { card, head, job } => take(c, card, head, job.as_str()),
        // An older build's: no join is taken now.
        ActionWhat::TakeJoin { .. } => Ok(json!({})),
        ActionWhat::MoveBase { card, project, base, target, .. } => {
            let p = c.project(project)?;
            let m = c.mirror(project)?;
            checkout::move_base(&c.home, Path::new(&p.repo), &m, card.as_str(), base, target.as_str()).map_err(failed)
        }
        ActionWhat::BackupPush { project, base, remote, .. } => {
            let p = c.project(project)?;
            checkout::push_backup(&c.home, Path::new(&p.repo), base, remote).map_err(failed)
        }
        ActionWhat::RemoveClone { card, run, keep_proofs } => {
            let dir = c.home.clones().join(card.as_str());
            let current = c.card.as_ref().is_none_or(|k| k.run == *run || !k.stage.is_live());
            // The handoff goes to the records first: a copy that fails keeps the clone, for a retry.
            let handoff = if current { preserve(c, card, *run, &dir)? } else { None };
            // The mock likewise: a later run carries on from it (fac-591).
            let mock = if current { super::mock_kept::keep(&c.home, card, *run, &dir).map_err(failed)? } else { None };
            if current {
                let aside = c.home.proofs().join(card.as_str());
                super::review_proofs::remove_clone(&dir, &aside, *keep_proofs)
                    .map_err(|e| failed(format!("{}: {e}", dir.display())))?;
            }
            if current {
                remove_trains(&c.home, card);
            }
            let unmerged = c.card.as_ref().filter(|_| current).map(|k| unmerged(c, k)).unwrap_or_default();
            Ok(json!({"removed": current, "unmerged": unmerged, "handoff": handoff, "mock": mock}))
        }
        ActionWhat::RemoveTrain { card, clone } => remove_train(&c.home.trains().join(format!("{card}-{clone}"))),
        ActionWhat::CreateBoardCard { .. } | ActionWhat::ChangeBoardCard { .. } => super::act_board::run(c),
        ActionWhat::Push { .. }
        | ActionWhat::OpenPr { .. }
        | ActionWhat::Comment { .. }
        | ActionWhat::ReplyComment { .. }
        | ActionWhat::ResolveComment { .. }
        | ActionWhat::Merge { .. } => super::act_forge::run(c),
        ActionWhat::Door(_) => crate::door::call::run(&c.home, &c.bin, &c.action, &c.door_redactor(), &c.stop),
    }
}

// The clone's handoff, copied into its project's records: where it is, and what it holds. A copy
// recorded whole was made before a removal that was cut short, and is what the clone held.
fn preserve(c: &Call, card: &CardId, run: u32, clone: &Path) -> Result<Option<Value>, ActionFailure> {
    let to = c.home.records().join(c.project.as_str()).join(mork_core::memory::handoff_dir(card, run));
    let aside = c.home.handoffs().join(card.as_str());
    let copied = match super::handoff::recorded(&to) {
        Ok(None) => match super::handoff::settle_aside(&aside, clone, &to) {
            Ok(None) if clone.exists() => super::handoff::preserve(clone, &to),
            other => other,
        },
        other => other,
    };
    let copied = copied.map_err(|e| failed(format!("{card}'s handoff: {e}")))?;
    // Only a copy filed whole lets the one aside go: a failure above keeps it, for the retry.
    super::handoff::drop_aside(&aside).map_err(|e| failed(format!("{card}'s handoff: {e}")))?;
    Ok(copied.map(|h| json!({"dir": to, "files": h.files, "skipped": h.skipped})))
}

fn remove_train(dir: &Path) -> Result<Value, ActionFailure> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
    }
    Ok(json!({"removed": true}))
}

// The card's train clones that outlived their candidates (a passed one's, kept to its landing):
// what can't go now goes at the engine's next start (fac-68).
fn remove_trains(home: &crate::paths::Home, card: &CardId) {
    let mine = format!("{card}-");
    for e in std::fs::read_dir(home.trains()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.strip_prefix(&mine).is_some_and(|tag| tag.bytes().all(|b| b.is_ascii_digit())) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Fetches the project into its mirror: the base's tip, the branches, and which cards' heads
/// already hold the tip.
pub fn fetch(c: &Call, project: &str) -> Result<Value, ActionFailure> {
    let p = c.project(project)?;
    let m = c.mirror(project)?;
    let (from, env) = source(&p).map_err(failed)?;
    mirror::fetch(&c.home, &m, &from, &env).map_err(|e| if p.forge.hosted() { git_failure(e) } else { failed(e) })?;
    let heads = mirror::heads(&c.home, &m);
    let tip_of = |b: &str| heads.iter().find(|h| h.name == b).and_then(|h| h.commit.clone());
    let tip = tip_of(&p.base).ok_or_else(|| failed(format!("{project} has no branch {}", p.base)))?;
    let contains: serde_json::Map<String, Value> = c
        .heads
        .iter()
        .map(|(card, head)| (card.as_str().to_owned(), json!(mirror::is_ancestor(&c.home, &m, &tip, head.as_str()))))
        .collect();
    let tips: BTreeMap<&String, String> = c.branches.iter().filter_map(|b| Some((b, tip_of(b)?))).collect();
    let branches: Vec<&String> = heads.iter().map(|h| &h.short).collect();
    Ok(json!({"base_tip": tip, "branches": branches, "contains": contains, "tips": tips}))
}

// A full card landed on its PR: its head's commits (`refs/cards/<card>`) that neither the base, its
// branch, nor what it pushed last holds, the last standing for a branch Bitbucket deleted (fac-171).
fn unmerged(c: &Call, k: &Card) -> Vec<String> {
    if k.mode != Mode::Full || k.stage != Stage::Done || k.pr.is_none() {
        return Vec::new();
    }
    let (Ok(p), Ok(m)) = (c.project(&k.project), c.mirror(&k.project)) else { return Vec::new() };
    let branch = k.branch.clone().unwrap_or_else(|| k.id.as_str().to_owned());
    let mut not = vec![format!("refs/heads/{}", p.base), format!("refs/heads/{branch}")];
    not.extend(k.pushed.iter().map(|x| x.0.clone()));
    mirror::past(&c.home, &m, &format!("refs/cards/{}", k.id), &not).unwrap_or_default()
}

fn take(c: &Call, card: &CardId, head: &Sha, job: &str) -> Result<Value, ActionFailure> {
    let k = c.card()?;
    let p = c.project(&k.project)?;
    let m = c.mirror(&k.project)?;
    let bundle = c.home.job(job).join("io/out.bundle");
    let r = format!("refs/cards/{card}");
    let before = mirror::rev(&c.home, &m, &r);
    let got = mirror::bundle_in(&c.home, &m, &bundle, &r).map_err(failed)?;
    if got != head.0 {
        return Err(failed(format!("the bundle holds {got}, not the head {head} its status named")));
    }
    // A train's candidate is never in the card's branch: the head it was made of is (fac-68).
    let pushed = k.pushed_head.as_ref().or(k.pushed.as_ref());
    let rewrites = pushed.is_some_and(|pushed| !mirror::is_ancestor(&c.home, &m, pushed.as_str(), &got));
    if rewrites {
        match &before {
            Some(b) => mirror::set_ref(&c.home, &m, &r, b).map_err(failed)?,
            None => {
                let _ = git::git(&c.home, &m, &["update-ref", "-d", &r], std::time::Duration::from_secs(30));
            }
        }
    }
    let tip =
        mirror::rev(&c.home, &m, &format!("refs/heads/{}", p.base)).ok_or_else(|| failed("the mirror has no base"))?;
    let fork = mirror::merge_base(&c.home, &m, &got, &tip).map_err(failed)?;
    let (changed, lines) = mirror::changed(&c.home, &m, &fork, &got).map_err(failed)?;
    let contents = mirror::contents(&c.home, &m, &fork, &got).map_err(failed)?;
    let t = Taken {
        head: Sha::new(got),
        fork: Sha::new(fork),
        changed,
        lines,
        rewrites_pushed: rewrites,
        base_tip: Sha::new(tip),
        contents,
    };
    serde_json::to_value(t).map_err(|e| failed(e.to_string()))
}
