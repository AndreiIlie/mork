//! What the engine's jobs and actions for a card report back to `core` (section 4): typed, so
//! `core` decides on facts and never on files.

use serde::{Deserialize, Serialize};

use crate::ids::{AgentKey, CardId, LeaseId, Sha};

/// Why a job of no agent's turn runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "purpose", rename_all = "kebab-case")]
pub enum Purpose {
    /// A seat's wall-test file, outside every wall while its requesting connection lives (fac-480).
    WallTests { card: CardId, seat: AgentKey, lease: LeaseId, command: Vec<String> },
    /// A card's clone, from a bundle of the base's tip, onto its branch (4.2.2).
    Clone { card: CardId, at: Sha, branch: String },
    /// The project's `install`, in the new clone. A review card's carries what its clone's end
    /// read, for its round to be asked once the install ran (fac-153).
    Install {
        card: CardId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cloned: Option<Box<Cloned>>,
    },
    /// With `recover`, the fork point comes in from the mirror first (fac-12); `base_in` brings the
    /// base's tip in (4.2.4), and `refresh` brings it in for no join, bundling no head (fac-333).
    Bundle {
        card: CardId,
        base_in: Option<Sha>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recover: Option<Sha>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh: Option<Sha>,
    },
    /// The checks gate on a head (4.7).
    Check { card: CardId, head: Sha, fork: Sha },
    /// An older build's join of the base's tip into a head, tested in a scratch clone (4.11.2.3):
    /// none is started now, the merge queue incorporating the base in the card's clone (fac-10).
    JoinTest { card: CardId, head: Sha, tip: Sha },
    /// An older build's: the base's tip merged into a queued card's head, in its own clone (fac-10).
    Incorporate { card: CardId, head: Sha, tip: Sha },
    /// An older build's: the merge queue's final run on a card's head, in its own clone (fac-10).
    Final { card: CardId, head: Sha, tip: Sha, checks: bool, test: Option<String> },
    /// A merge train's candidate (fac-68): the card's head, on the commit it pushed last when it
    /// has one, with `on` merged in, made in a disposable clone of its own (`clone`, its tag).
    Candidate { card: CardId, head: Sha, on: Sha, pushed: Option<Sha>, clone: i64 },
    /// The candidate's install, owed checks against `on`, and suite; `test` binds its pass key.
    /// `unwalled` runs Mork's deferred wall tests outside every wall (fac-65).
    TrainRun {
        card: CardId,
        commit: Sha,
        on: Sha,
        clone: i64,
        checks: bool,
        test: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        unwalled: bool,
    },
    /// The project's suite on a review card's head, before its review (4.4.1, 4.16.2).
    Suite { card: CardId, head: Sha },
    /// A build of Mork's own repo at a commit: a card of it landed there (1.8.4, M3.22).
    Build { commit: Sha },
}

impl Purpose {
    /// What names it while it is owed: its own JSON, the same for the same job.
    pub fn key(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn card(&self) -> Option<&CardId> {
        match self {
            Self::Clone { card, .. }
            | Self::Install { card, .. }
            | Self::Bundle { card, .. }
            | Self::Check { card, .. }
            | Self::JoinTest { card, .. }
            | Self::Incorporate { card, .. }
            | Self::Final { card, .. }
            | Self::Candidate { card, .. }
            | Self::TrainRun { card, .. }
            | Self::WallTests { card, .. }
            | Self::Suite { card, .. } => Some(card),
            Self::Build { .. } => None,
        }
    }
}

/// Result of projecting a review card's preceding candidate onto a moved base (fac-628).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum RebasedPrevious {
    /// `refs/rounds/<n>-on-base` was created; `conflicted` tracks `merge-tree` exit 1 even with no listed paths.
    Projected {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        conflicted: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        conflicts: Vec<String>,
    },
    /// The round's commit does not contain the moved base, so no merged base needed stripping.
    Unmerged,
}

/// A review card's clone as its Clone job's end read it: its commit (fac-153).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cloned {
    pub at: Sha,
    /// The numbers of the findings the operator resolved on the review card (fac-298).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved: Vec<u64>,
    /// The open findings' threads as this round read them, including failed reads (fac-448).
    #[serde(
        default,
        skip_serializing_if = "std::collections::BTreeMap::is_empty",
        deserialize_with = "crate::review::threads_in"
    )]
    pub threads: crate::review::Threads,
    /// Outcome of projecting the preceding candidate onto a moved base in the review clone (fac-628).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebased: Option<RebasedPrevious>,
}

/// A clone's state, from a bundle job's `io/status.json` (4.2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneStatus {
    pub branch: String,
    pub head: Option<Sha>,
    pub clean: bool,
    /// The paths that aren't clean.
    pub dirty: Vec<String>,
    /// Whether a bundle was made (none when the branch has no commit past the base tip).
    pub bundle: bool,
}

/// A head taken into the mirror (the `take` action's result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taken {
    pub head: Sha,
    pub fork: Sha,
    /// The files it changes against the fork point, and how many lines.
    pub changed: Vec<String>,
    pub lines: u64,
    /// It doesn't descend from the commit pushed last (4.2.3).
    pub rewrites_pushed: bool,
    /// The base's tip in the mirror now.
    pub base_tip: Sha,
    /// What it makes of each file it changes against the fork point (fac-95).
    #[serde(default)]
    pub contents: crate::review::Contents,
}

/// A run of the checks gate (4.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    pub head: Sha,
    pub exit: Option<i32>,
    /// `io/checks.json`, when it was written.
    pub report: Option<serde_json::Value>,
    /// The checks folder's commit, and whether it had uncommitted changes.
    pub folder_commit: String,
    pub folder_dirty: bool,
    /// Each finding's place in the clone, located (4.8.2): the path read, or why not.
    pub located: Vec<Result<String, String>>,
    /// The checks folder moved while they ran (4.7.7, M3.12).
    #[serde(default)]
    pub folder_moved: bool,
}

/// A train's candidate as its clone made it (fac-68): the paths its merges conflicted in; else the
/// commit made, in the mirror, and its tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    pub conflicts: Vec<String>,
    /// The conflict is the head's merge into the commit pushed last, not the base tip's (fac-155).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub into_pushed: bool,
    pub commit: Option<Sha>,
    pub tree: Option<String>,
    /// What the commit makes of each file it changes past what it was made on, read in the mirror
    /// (fac-95).
    #[serde(default)]
    pub contents: crate::review::Contents,
    /// A PR's changes since its last reviewed source head, read in the mirror.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_changes: Option<crate::review::Contents>,
}

/// A queued card's clone as the final run read it, before or after (fac-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    pub head: Option<Sha>,
    pub tree: Option<String>,
    /// No tracked file differs from the head.
    pub tracked_clean: bool,
    /// Nor is any file untracked, ignored ones aside.
    pub clean: bool,
}

/// The merge queue's final run (fac-10): the checkout around it, the checks' run, the suite's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalRun {
    pub head: Sha,
    pub before: Checkout,
    pub after: Checkout,
    pub checks: Option<CheckRun>,
    /// Whether the suite ran, its exit (none when a signal ended it) and its output's tail, led by
    /// its failures and followed by its whole output's path when it failed (fac-144).
    pub ran: bool,
    pub exit: Option<i32>,
    pub output: String,
    /// The phase the suite actually ran with; absent in results from before fac-65.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Its failing tests by file, when a reader recognised its output (fac-164).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failing: Option<Vec<Failing>>,
    /// Its whole log copied into each card's clone its failure is told in: the copy's path, by card.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub logs: std::collections::BTreeMap<CardId, String>,
}

/// A failing test, under the file that holds it: Jest's test file, or the Rust test binary's
/// source file. A file failing as a whole names the test its reader gives it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Failing {
    pub file: String,
    pub test: String,
}

/// A PR as the poller or the re-read before a merge reads it (4.11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSeen {
    pub pr: u64,
    pub state: PrState,
    pub head: Sha,
    /// The operator's approval stands on the current head (4.11.1).
    pub approved: bool,
    /// For a merged PR: the merge commit's parents, when the mirror holds it.
    pub merge_parents: Vec<Sha>,
    /// For a merged PR: its merge commit, the base's new tip (1.8.4, 4.12.4; M4.21).
    #[serde(default)]
    pub merge_commit: Option<Sha>,
    /// The branch it merges into, as Bitbucket read it (fac-95).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
}

/// A PR's state on Bitbucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PrState {
    Open,
    Merged,
    Declined,
    Superseded,
}

/// A colleague's open PR (4.16.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPr {
    #[serde(default)]
    pub review_requested: bool,
    pub pr: u64,
    pub author: String,
    pub title: String,
    pub branch: String,
    pub head: Sha,
    /// Opened by the token's own account.
    pub ours: bool,
    /// The branch it merges into, its last update (Bitbucket's `updated_on`) and its page, for
    /// `prs` (fac-28); a read before them has none.
    #[serde(default)]
    pub dest: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub link: String,
    /// Its description as Bitbucket gave it (fac-292).
    #[serde(default)]
    pub description: String,
    /// The Azure DevOps tickets its title, description and branch refer to, as read with the PR.
    #[serde(default)]
    pub tickets: Vec<Ticket>,
}

/// An Azure DevOps work item a PR refers to (fac-292): its text as the ticket's author wrote it,
/// markup stripped; an empty field is one the ticket doesn't have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repro_steps: String,
}

/// How an outward action failed (2.5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "kebab-case")]
pub enum ActionFailure {
    /// An error from Bitbucket or Git, in their words.
    Failed { words: String },
    /// A board change refused by its check of the file's content, before anything was replaced.
    BoardUnwritten { words: String },
    /// The leak check (3.8): the kind of secret and where.
    Leak { words: String },
    /// The internals check (3.10.6): the word and where.
    Internals { words: String },
    /// A push refused as not a fast-forward: origin's branch is at `at`.
    NotFastForward { at: Option<Sha> },
    /// The Bitbucket token was refused before or during a call.
    Token { words: String },
    /// The PR is gone (404).
    Gone { words: String },
    /// A door read whose server didn't take the kill of its query (fac-184): it may still run there,
    /// on the session recorded for it.
    Unstopped { words: String, session: crate::door_plan::Session },
    /// A guard no longer holds right before the call (2.5.8): no call was made. A merge's carries
    /// the PR as read, boxed for its size, and is held; an older build's, with no destination, is reread.
    Guard { words: String, pr: Option<Box<PrSeen>> },
    /// An approved merge's destination moved from the base its review's run merged in (fac-111): no
    /// call was made, and the PR goes through the merge train again on `tip`, the tip read then.
    Moved { words: String, tip: Sha },
    /// Bitbucket refused a merge for its conflicts (F-B7, fac-98): the destination's tip as the
    /// mirror fetched it then, and the files both sides change, as far as the mirror could tell.
    Conflict {
        words: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tip: Option<Sha>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
}

/// What the engine found of a `started` action after a crash (2.5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "found", rename_all = "kebab-case")]
pub enum Found {
    /// It happened: what it did.
    Done { result: serde_json::Value },
    /// It didn't: run it.
    NotDone,
    /// The world doesn't say clearly: why.
    Unclear { words: String },
    /// A door read cut whose server didn't take the kill of its query (fac-184): why, and the
    /// session it may still run on.
    Unstopped { words: String, session: crate::door_plan::Session },
}
