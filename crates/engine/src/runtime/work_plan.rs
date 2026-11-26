use std::path::PathBuf;

use mork_types::card::{Card, Mode};
use mork_types::config::ProjectConfig;
use mork_types::job::JobKind;
use mork_types::work::Purpose;
use mork_types::{CardId, Sha};

use super::Engine;
use super::work::{Plan, fresh_dir, json_of, plan};
use crate::git::mirror;
use crate::jobs::work::{BundleIn, Order, RebasePrevious};
use crate::paths::Home;
use crate::random;

#[derive(Default)]
struct PreviousRound {
    bundle: Option<BundleIn>,
    extra: Vec<BundleIn>,
    rebase: Option<RebasePrevious>,
}

pub(super) struct Worktree {
    pub home: Home,
    pub card: Card,
    pub project: ProjectConfig,
    pub argv: Vec<String>,
}

impl Engine {
    pub(super) fn worktree(&self, purpose: &Purpose) -> Result<Worktree, String> {
        let c = self.card(purpose.card().ok_or("the work has no card")?)?;
        let project = self.project(&c.project)?;
        Ok(Worktree { home: self.opts.home.clone(), card: c, project, argv: self.work_argv() })
    }
}

impl Worktree {
    pub(super) fn prepare(&self, purpose: &Purpose, exports: &mut Vec<PathBuf>) -> Result<Plan, String> {
        match purpose {
            Purpose::Clone { card, at, branch } => self.plan_clone(card, at.as_str(), branch, exports),
            Purpose::Bundle { card, base_in, recover, refresh } => {
                self.plan_bundle(purpose, card, base_in.as_ref().or(refresh.as_ref()), recover.as_ref(), exports)
            }
            Purpose::Candidate { .. } => self.plan_candidate(purpose, exports),
            _ => Err("the work needs no mirror preparation".into()),
        }
    }

    // Beside the mirror for a job to copy into its `io/`; it goes once the job's folder holds it (fac-175).
    fn bundle_of(
        &self,
        project: &str,
        name: &str,
        commit: &str,
        exports: &mut Vec<PathBuf>,
    ) -> Result<(String, String), String> {
        let m = mirror::ensure(&self.home, project)?;
        let dir = self.home.mirrors().join(format!("{project}.bundles"));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let out = dir.join(format!("{name}-{}.bundle", random::salt()));
        exports.push(out.clone());
        let r = mirror::bundle_out(&self.home, &m, name, commit, &out)?;
        Ok((out.display().to_string(), r))
    }

    fn plan_clone(&self, card: &CardId, at: &str, branch: &str, exports: &mut Vec<PathBuf>) -> Result<Plan, String> {
        let c = &self.card;
        let p = &self.project;
        let dir = self.home.clones().join(card.as_str());
        // Reviewers reuse their original proofs to verify a later round's fixes (fac-276).
        if c.mode == Mode::Review {
            super::review_proofs::fresh_review_dir(&self.home, card, &dir)?;
        } else {
            super::mock_kept::forget_restore(&self.home, card)?;
            fresh_dir(&dir)?;
        }
        let (bundle, r) = self.bundle_of(&c.project, &format!("{card}-start"), at, exports)?;
        // A later run's clone starts on its branch, the base's tip beside it (fac-111).
        let tip = c.base_tip.as_ref().filter(|t| c.mode != Mode::Review && t.as_str() != at);
        let into = if c.mode == Mode::Review || tip.is_some() {
            format!("refs/remotes/origin/{branch}")
        } else {
            format!("refs/remotes/origin/{}", p.base)
        };
        let d = dir.display().to_string();
        let mut pl = plan(
            &Purpose::Clone { card: card.clone(), at: mork_types::Sha::new(at), branch: branch.to_owned() },
            JobKind::Bundle,
            self.argv.clone(),
            &d,
            &c.project,
        )
        .rw(d);
        let base = match tip {
            Some(t) => Some(self.bundle_in(&mut pl, exports, "base", t, format!("refs/remotes/origin/{}", p.base))?),
            None => None,
        };
        let prev = self.previous_round_in(&mut pl, exports)?;
        let order = Order::Clone {
            from: BundleIn { bundle: "{io}/start.bundle".into(), r#ref: r, into: into.clone() },
            branch: branch.to_owned(),
            start: into,
            exclude: p.copy.clone(),
            base,
            local_base: local_base(c, branch, &p.base),
            previous: prev.bundle,
            extra: prev.extra,
            rebase_previous: prev.rebase,
        };
        pl.files.insert("work.json".into(), json_of(&order)?);
        pl.copies.insert("start.bundle".into(), bundle);
        Ok(pl)
    }

    // A later round's clone keeps the round before it as `refs/rounds/<n>` (fac-251) and, when both
    // candidates hold their bases and the base moved, projects it as `refs/rounds/<n>-on-base` (fac-628).
    fn previous_round_in(&self, pl: &mut Plan, exports: &mut Vec<PathBuf>) -> Result<PreviousRound, String> {
        let c = &self.card;
        let Some(round) = c.review.as_deref().and_then(|r| r.open()) else { return Ok(PreviousRound::default()) };
        let Some(before) = mork_core::reviews::rounds::previous(c, round.n) else {
            return Ok(PreviousRound::default());
        };
        let into = mork_core::reviews::rounds::round_ref(before.n);
        let bundle = Some(self.bundle_in(pl, exports, "previous", &before.commit, into.clone())?);
        if before.base == round.base {
            return Ok(PreviousRound { bundle, ..PreviousRound::default() });
        }
        let m = mirror::ensure(&self.home, &c.project)?;
        let (has_prev, has_curr) = (
            mirror::is_ancestor(&self.home, &m, before.base.as_str(), before.commit.as_str()),
            mirror::is_ancestor(&self.home, &m, round.base.as_str(), round.commit.as_str()),
        );
        let mut extra = Vec::new();
        if !has_prev && !mirror::is_ancestor(&self.home, &m, before.base.as_str(), round.commit.as_str()) {
            extra.push(self.bundle_in(pl, exports, "prev-base", &before.base, "refs/mork/prev-base".into())?);
        }
        if !has_curr && !mirror::is_ancestor(&self.home, &m, round.base.as_str(), before.commit.as_str()) {
            extra.push(self.bundle_in(pl, exports, "new-base", &round.base, "refs/mork/new-base".into())?);
        }
        // Without the new base inside the candidate, projecting onto it hides branch edits (fac-628).
        if !has_curr {
            pl.files.insert("rebased.json".into(), r#"{"status":"unmerged"}"#.into());
            return Ok(PreviousRound { bundle, extra, rebase: None });
        }
        if !has_prev {
            return Ok(PreviousRound { bundle, extra, rebase: None });
        }
        let rebase = Some(RebasePrevious {
            prev_base: before.base.0.clone(),
            prev_ref: into,
            new_base: round.base.0.clone(),
            into: mork_core::reviews::rounds::rebased_round_ref(before.n),
        });
        Ok(PreviousRound { bundle, extra, rebase })
    }

    // Its commits past the fork point, which the clone holds; never past the mirror's tip, which
    // it may not (fac-12). A recovery's fork goes beside the base, so a join's import stays.
    fn plan_bundle(
        &self,
        purpose: &Purpose,
        card: &CardId,
        base_in: Option<&Sha>,
        recover: Option<&Sha>,
        exports: &mut Vec<PathBuf>,
    ) -> Result<Plan, String> {
        let c = &self.card;
        let p = &self.project;
        let dir = self.home.clones().join(card.as_str()).display().to_string();
        let past = recover.or(c.fork.as_ref()).or(c.base_tip.as_ref()).ok_or("the card has no fork point")?;
        let mut pl = plan(purpose, JobKind::Bundle, self.argv.clone(), &dir, &c.project).rw(dir);
        let base_in = match base_in {
            Some(tip) => {
                Some(self.bundle_in(&mut pl, exports, "base", tip, format!("refs/remotes/origin/{}", p.base))?)
            }
            None => None,
        };
        let fork_in = match recover {
            Some(fork) => Some(self.bundle_in(&mut pl, exports, "fork", fork, "refs/mork/fork".into())?),
            None => None,
        };
        let local_base = base_in.as_ref().and_then(|_| local_base(c, c.branch.as_deref().unwrap_or_default(), &p.base));
        let base_only = matches!(purpose, Purpose::Bundle { refresh: Some(_), .. });
        let order = Order::Bundle { base_tip: past.0.clone(), base_in, diff: false, fork_in, local_base, base_only };
        pl.files.insert("work.json".into(), json_of(&order)?);
        Ok(pl)
    }

    // A commit of the mirror, copied into the job's `io/` as `<what>.bundle`, for it to fetch into `into`.
    pub(super) fn bundle_in(
        &self,
        pl: &mut Plan,
        exports: &mut Vec<PathBuf>,
        what: &str,
        commit: &Sha,
        into: String,
    ) -> Result<BundleIn, String> {
        let c = &self.card;
        let (bundle, r) = self.bundle_of(&c.project, &format!("{}-{what}", c.id), commit.as_str(), exports)?;
        pl.copies.insert(format!("{what}.bundle"), bundle);
        Ok(BundleIn { bundle: format!("{{io}}/{what}.bundle"), r#ref: r, into })
    }

    // An empty folder of its own, the head, the commit pushed last and `on` bundled from the mirror.
    pub(super) fn plan_candidate(&self, purpose: &Purpose, exports: &mut Vec<PathBuf>) -> Result<Plan, String> {
        let Purpose::Candidate { card, head, on, pushed, clone } = purpose else {
            return Err("not a candidate".into());
        };
        let c = &self.card;
        let p = &self.project;
        let dir = self.home.trains().join(format!("{card}-{clone}"));
        fresh_dir(&dir)?;
        let d = dir.display().to_string();
        let mut pl = plan(purpose, JobKind::Bundle, self.argv.clone(), &d, &c.project).rw(d);
        let head_in = self.bundle_in(&mut pl, exports, "head", head, "refs/mork/head".into())?;
        let on_in = self.bundle_in(&mut pl, exports, "on", on, "refs/mork/on".into())?;
        let pushed_in = match pushed {
            Some(x) => Some(self.bundle_in(&mut pl, exports, "pushed", x, "refs/mork/pushed".into())?),
            None => None,
        };
        let branch = c.branch.clone().unwrap_or_else(|| card.as_str().to_owned());
        let order = Order::Candidate {
            head: head_in,
            on: on_in,
            pushed: pushed_in,
            branch,
            base: p.base.clone(),
            exclude: p.copy.clone(),
        };
        pl.files.insert("work.json".into(), json_of(&order)?);
        Ok(pl)
    }
}

// A review clone's remote branch is the candidate, not the base (fac-165); no local base there,
// nor where the card's own branch has the base's name.
fn local_base(c: &Card, branch: &str, base: &str) -> Option<String> {
    (c.mode != Mode::Review && branch != base).then(|| base.to_owned())
}
