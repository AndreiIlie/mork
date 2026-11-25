//! Cards (section 4): how a card runs, where it stands, and its record.

use serde::{Deserialize, Serialize};

pub use crate::landing::{Candidate, Landing};
pub use crate::plan::{Deliverable, Plan, WHOLE_CARD};

/// How a card runs (section 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Full,
    /// An older build's trivial card (fac-24), kept for its records: no card starts light (fac-130).
    Light,
    Report,
    /// A colleague's PR, reviewed (4.16).
    Review,
}

/// Where a card stands in its pipeline (4.4.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    #[default]
    Starting,
    Brief,
    Build,
    Checks,
    Review,
    Second,
    /// Also reads an older state's preview stage (fac-411), admitted to verification by startup recovery.
    #[serde(alias = "preview")]
    Ready,
    Merging,
    Report,
    Done,
    Closed,
}

/// A card's record (section 4): one run of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: crate::ids::CardId,
    pub project: crate::ProjectId,
    pub title: String,
    pub description: String,
    pub acceptance: Vec<String>,
    pub mode: Mode,
    pub stage: Stage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookback: Option<crate::lookback::Run>,
    /// The run, from 1 (4.13.6).
    pub run: u32,
    pub paused: bool,
    /// The card's preview port (4.3.4), once it has one.
    pub port: Option<u16>,
    /// The approved brief, once there is one (4.5), and its risks.
    pub brief: Option<String>,
    /// The brief's approval as the engine recorded it (fac-16, fac-24, fac-440). None on a brief an
    /// older build approved: no evidence of agreement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    pub risks: Vec<crate::reply::Risk>,
    /// The brief drafted and not yet approved: the `brief-ready` hand's message.
    pub draft: Option<String>,
    /// The draft's typed sketch, when its recap gave one (fac-696): what its approve ask shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<Sketch>,
    /// The draft is offered for approval once its foreman's challenge found no objection (fac-558).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recap: bool,
    /// The checks' output on the head they last passed (4.7.3): what a review round reads (4.8.5).
    pub checks_output: Option<String>,
    /// The card's branch (3.10.1), set when it starts; a later run keeps it (fac-111).
    pub branch: Option<String>,
    /// Prior unpublished head; published runs resume at the fetched branch tip (fac-111). None initially.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<crate::ids::Sha>,
    /// The base's tip the clone was made at, or the latest joined in.
    pub base_tip: Option<crate::ids::Sha>,
    /// The latest head taken into the mirror (4.2.3), with its fork point and changes.
    pub head: Option<crate::ids::Sha>,
    pub fork: Option<crate::ids::Sha>,
    /// The bundle its run recorded last, and the head that bundle read (fac-97): only that head is
    /// taken, and answers a `built`. None on a card an older build started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundled: Option<Bundled>,
    /// The bundles and checks of a submission its reopened mock withdrew (fac-591): their ends and takes count for
    /// nothing, kept until each settles or returns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<crate::ids::JobId>,
    pub changed: Vec<String>,
    pub lines: u64,
    /// Where the gates stand for the current head.
    pub gates: Gates,
    /// Fix requests sent to the foreman in this run: none is held; from the third on,
    /// each is said in the room and in main.
    pub revisions: u32,
    /// The commit pushed last (3.10), and the PR (Bitbucket).
    pub pushed: Option<crate::ids::Sha>,
    /// The push before the latest one: Bitbucket may still read it just after publication (fac-204).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_push: Option<crate::ids::Sha>,
    /// The card's head that commit was made of: a train's candidate is never in the card's
    /// branch, so a later head must descend from this one (fac-68). None when an older build pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_head: Option<crate::ids::Sha>,
    /// The heads our PR was published at once their verification passed (fac-375): the poller reviews any other head
    /// of it. Each recorded before its push, kept through later fixes; none when an older build published it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_pushes: Vec<crate::ids::Sha>,
    pub pr: Option<u64>,
    /// A PR carried from an earlier run that no read has yet found open for this one (fac-171).
    #[serde(default)]
    pub pr_unconfirmed: bool,
    /// What the earlier run had pushed when its PR was carried: a push since is this run's (fac-171).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carried_push: Option<crate::ids::Sha>,
    /// The join owed, if one is (4.12).
    pub join: Option<Join>,
    /// A train failure's request to the foreman, held until its tip reaches the clone (fac-144).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_import: Option<FixImport>,
    /// The base's newest tip a refresh brings into the clone, no join's (fac-333).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<Refresh>,
    /// Its candidate's failure news, until it is told (fac-164).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub news: Option<crate::suites::SuiteNews>,
    /// Cards sharing files with it (4.15).
    pub overlaps: std::collections::BTreeMap<crate::ids::CardId, Vec<String>>,
    /// Why it was closed (4.13.6).
    pub closed: Option<String>,
    pub started_ms: i64,
    /// A job of the card's that failed, was stopped or cut: `resume` runs it again (4.13.2).
    pub failed_job: Option<crate::work::Purpose>,
    /// Cut jobs in a row (1.4.6.5).
    pub job_cuts: u32,
    /// A PR alarm raised (4.11.6): raised once, answered by `close`, or by Andrei's or an orchestrator's `ack` (fac-157).
    pub alarm: bool,
    /// Where its landing stands (4.11.2).
    pub landing: Option<Landing>,
    /// A review card's findings, as its foreman's review-fail gave them (4.16.2).
    pub findings: Vec<crate::reply::Finding>,
    /// The first message seq of this run: a new run's sessions read no news from before it.
    pub first_seq: u64,
    /// Its deliverables and each one's tasks (fac-36, fac-128). None on a card an older build ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    /// What its head makes of each file it changes past its fork (fac-95).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub contents: crate::review::Contents,
    /// A review card's review: what it reviews, its rounds, and what they approved (fac-95).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Box<crate::review::Review>>,
    /// Seat questions and non-visual plans are automatically given to the project's orchestrator (fac-474, fac-669).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delegated: bool,
    /// The project's orchestrator answers departures and reversals too, instead of leaving them to Andrei (fac-474, fac-669).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub self_sustained: bool,
    /// Andrei turned its preview on (fac-222): it runs, an app's start bringing it back, until he
    /// stops it or the card ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
    /// In visual mode (fac-413, fac-636): a full card's mock made at brief and approved with its plan, on
    /// Andrei's switch or an orchestrator's start; kept across restarts and later runs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub visual: bool,
    /// This run has no planner (fac-616): its foreman holds the specification, at brief and after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_planner: bool,
    /// A visual card's mock (fac-413, fac-636): its baseline, made at brief, or reopened at build; this run's only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mock: Option<Mock>,
    /// Its foreman or Andrei sent it back to brief after its plan's approval (fac-638): this run's only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebrief: Option<Rebrief>,
    /// The seq of the engine's request for the foreman's challenge of the current recap (fac-558): only a reply
    /// whose turn read it counts. None once answered, withdrawn or approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<u64>,
    /// The seqs of the hands kept as recaps of the plan in brief, the latest last (fac-618): a foreman's news shortens
    /// only these. Emptied at approval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recaps: Vec<u64>,
    /// The board cards this run carries (fac-563), in the order its start named them: they share its plan, room, clone,
    /// branch, review and merge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carries: Vec<Carried>,
    /// The `started` writes this run's first turn waits for (fac-563): empty until its clone is made. None on an
    /// older build's start, whose own card's write moves it on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaits: Option<Vec<crate::ids::ActionId>>,
    /// A boards read found this closed run's card closed (fac-563): what it carries is free, whatever later reads miss.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub released: bool,
    /// The seq of its stall's one nudge (fac-587): none posted again until a check finds the card moving for a
    /// reason other than the nudged seat's own turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stalled: Option<u64>,
    /// A failed review round waiting for triage before waking the foreman, when delegation is off or on a delegated stalemate (fac-680, fac-689, fac-694).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage: Option<crate::review::PendingTriage>,
}

/// A card a run carries (fac-563), with its words as the run last saved them: what an edit's announcement compares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carried {
    pub id: crate::ids::CardId,
    pub title: String,
    pub description: String,
    pub acceptance: Vec<String>,
    /// Its board file when it joined: its status writes go there when the boards' last read missed it.
    #[serde(default)]
    pub file: String,
}

/// A visual card's mock step (fac-258): its mock made, served and shown to Andrei before its build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mock {
    pub step: MockStep,
    /// The mock was offered for approval (fac-484); absent on older records until carry-over.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub offered: bool,
    /// The mock host's port, from the factory's port range, apart from its preview port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Andrei reopened it during the run's build or later (fac-591): how many notes its card had then, so the
    /// decisions recorded since are those numbered above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopened: Option<usize>,
    /// The seq of the engine's request to continue the build after a reopened mock (fac-591): only a foreman turn
    /// that read it submits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued: Option<u64>,
}

/// A card sent back to brief in its run (fac-638): its build held until its new plan's approve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rebrief {
    /// How many notes its card had then: the decisions recorded since are those numbered above it.
    pub notes: usize,
    /// The seq of the engine's request to agree the new plan: only a recap of a turn that read it counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<u64>,
    /// The seq of the engine's request to continue the build: only a foreman turn that read it submits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued: Option<u64>,
}

/// Where a mock step stands (fac-413, fac-636): the plan's approve, a reopen's, or visual mode off ends it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MockStep {
    /// The mock is made and changed as Andrei comments, until his `approve`.
    #[default]
    Open,
    /// Andrei's `approve` ended it.
    Approved,
    /// Visual mode was turned off during it: ended without the mock.
    Skipped,
    /// A visual card's baseline seat reproduces today's pages before its planner's turn (fac-636).
    Baseline,
}

/// A recap's sketch (fac-696), as its holder's hand typed it: what changes, then the code's shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sketch {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_shape: Vec<String>,
}

impl Sketch {
    /// Its two lists under their headings, one `- ` line each, for readers of words.
    pub fn words(&self) -> String {
        let part = |head: &str, lines: &[String]| {
            (!lines.is_empty())
                .then(|| format!("{head}\n{}", lines.iter().map(|l| format!("- {l}")).collect::<Vec<_>>().join("\n")))
        };
        [part("What changes", &self.changes), part("Code shape", &self.code_shape)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Who approved a card's specification, how and when (fac-16), or who started its build with
/// nothing to clarify (fac-24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    /// Andrei, a delegated card's foreman, or its project's orchestrator holding its open plan ask.
    /// Older clear starts name the agent holding the specification; they approved nothing.
    pub by: crate::state::Sender,
    pub via: ApprovedVia,
    pub at_ms: i64,
}

/// How a specification was approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovedVia {
    /// A plan's approval: `approve`, or its foreman's on a delegated card (fac-440).
    Approve,
    /// His yes to the recap in the card's room, read until fac-335: kept for approvals so recorded.
    Chat,
    /// No approval: the agent holding the specification found nothing left to clarify, and its
    /// reply, asking nothing, started the build (fac-24).
    Clear,
}

/// The gates of a card's current head (4.4.3, 4.7.7, 4.8): each holds the head it is for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gates {
    /// The checks running, on this head.
    pub checking: Option<crate::ids::Sha>,
    /// This head's untrusted checks wait for `resume` (fac-484); absent until they fail or carry over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks_waiting: Option<crate::ids::Sha>,
    /// The checks passed on this head, with the checks folder at this commit.
    pub checked: Option<(crate::ids::Sha, String)>,
    /// A review card's review was asked about this head.
    pub review_asked: Option<crate::ids::Sha>,
    /// This head passed its gates, its checks or none configured: what verification and the merge queue admit.
    pub passed: Option<crate::ids::Sha>,
    /// Andrei's approval stands on this head (Bitbucket, 4.11.1), approved at this time.
    pub approved: Option<(crate::ids::Sha, i64)>,
    /// The foreman answered `built` and the head is being taken.
    pub built_pending: bool,
    /// The checks wait for a test slot (4.9.7).
    pub slot_wait: Option<crate::work::Purpose>,
    /// The clone's tree was clean at its last bundle.
    pub clean: bool,
}

/// A run's bundle recorded last (fac-97).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundled {
    /// None until the run records one.
    pub job: Option<crate::ids::JobId>,
    /// The head past the base its clone held as it ended: none while it runs, after it failed, or
    /// with no commit.
    pub head: Option<crate::ids::Sha>,
}

/// A join owed by a card (4.12.1): recorded once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    pub why: String,
    pub base: String,
    pub tip: crate::ids::Sha,
    pub asked_of: Option<crate::ids::Sha>,
    /// Its request waits for its tip to reach the clone as origin/<base>, and a stopped card's
    /// for its `resume` (4.12.4, fac-97).
    pub waiting: bool,
}

/// The foreman's request after a train failure, posted once `tip`, the commit its candidate was
/// merged onto, is in the clone as origin/<base>, or its import failed (fac-144).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixImport {
    pub tip: crate::ids::Sha,
    pub base: String,
    pub headline: String,
    pub text: String,
}

/// A tip refreshed into the clone as origin/<base>, `done` once in; until then it waits for the
/// clone's import or refresh under way (fac-333).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refresh {
    pub tip: crate::ids::Sha,
    pub done: bool,
}

/// A card as its board file holds it (Backlog.md, F-K): what `start` and the prompts read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardCard {
    pub id: crate::ids::CardId,
    pub project: crate::ProjectId,
    pub title: String,
    pub description: String,
    pub acceptance: Vec<String>,
    pub status: String,
    /// Its labels: one `area:<name>` among them puts it in that area of the Board (fac-37).
    #[serde(default)]
    pub labels: Vec<String>,
    /// The board's done status: the last of its `config.yml`'s `statuses` (4.18).
    pub done_status: String,
    /// The file, relative to the board's `backlog/`.
    pub file: String,
    /// The file's whole text, as read: what a change of it is made from (M3.16).
    #[serde(default)]
    pub text: String,
    /// The board's id prefix (`config.yml`'s `task_prefix`), for a new card's id.
    #[serde(default)]
    pub prefix: String,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Brief => "brief",
            Self::Build => "build",
            Self::Checks => "checks",
            Self::Review => "review",
            Self::Second => "second",
            Self::Ready => "ready",
            Self::Merging => "merging",
            Self::Report => "report",
            Self::Done => "done",
            Self::Closed => "closed",
        }
    }

    pub fn is_live(self) -> bool {
        !matches!(self, Self::Done | Self::Closed)
    }
}

impl Card {
    /// The current delegation level: off, delegated or self-sustained.
    pub fn delegation(&self) -> &'static str {
        match (self.delegated, self.self_sustained) {
            (false, _) => "off",
            (true, false) => "delegated",
            (true, true) => "self-sustained",
        }
    }

    /// A run of a card, `starting`, with nothing made yet.
    pub fn new(id: crate::ids::CardId, project: &str, title: &str, mode: Mode, run: u32, now: i64) -> Self {
        Self {
            id,
            project: crate::ProjectId::new(project),
            title: title.to_owned(),
            mode,
            run,
            started_ms: now,
            bundled: Some(Bundled::default()),
            awaits: Some(Vec::new()),
            ..Self::default()
        }
    }
}
