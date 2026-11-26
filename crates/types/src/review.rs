//! Review state binds rounds and findings to file content, retaining approvals for unchanged files.
//! Changed content needs another round (fac-95, fac-111).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{ActionId, CardId, Sha};

/// What a change makes of each file it touches, by path: the file's mode and blob after it
/// (`100644 <blob>`), a deleted file's zeros. A rename is a deletion and an addition.
pub type Contents = BTreeMap<String, String>;

/// A review card's review: of a Mork card's branch in the train, or of a PR or a branch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// The Mork card whose branch it reviews in the train; none for a PR or a branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<CardId>,
    /// Opened by `review.merge`: approval merging remains enabled into the PR's destination.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge: bool,
    /// Started automatically: base PRs, or PRs titled with a project's configured `review_tag` (fac-269, fac-721).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto: bool,
    /// A PR's destination branch as the poller read it: its diff's base, and what it merges into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    /// The PR author's description, as context for what the PR means to do.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The Azure DevOps tickets the PR refers to, as the poller read them with its latest head (fac-292).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tickets: Vec<crate::work::Ticket>,
    /// The comments its runs asked, by their actions: its own results on the PR (fac-98).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<ActionId>,
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// What to review once the round under way has its verdict: the latest content asked while it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Round>,
    /// Each file's content a passed round approved.
    #[serde(default)]
    pub approved: Contents,
    /// The findings its rounds raised, each with its raiser and its comment (fac-111).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raised: Vec<Raised>,
    /// A PR entry's latest train run, ended: what an approval of its head merges on (fac-111).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<Finished>,
    /// The head an approval of the operator's was read on while its finished run had failed there: he is
    /// told once that it doesn't merge (fac-111).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Sha>,
    /// The first round of our card's plan agreed again after `brief` (fac-638): earlier rounds approve and carry
    /// nothing, and their failed verdicts pass nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afresh: Option<u32>,
}

/// A finding a round raised (fac-111): its raiser verifies its fix before its comment is resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raised {
    /// Its number on the review card, from 1: what `resolve` names.
    pub n: u32,
    pub round: u32,
    /// Who raised it: a reviewer's name (`reviewer-2`), or `foreman`.
    pub by: String,
    pub file: String,
    pub line: i64,
    pub summary: String,
    /// Its proof, for its comment asked in a later round when no run posted it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proof: String,
    /// Its comment's action, and the comment's id on Bitbucket once it is posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolved: bool,
    /// Resolved as one the operator's decision rules out: its thread is told so once its comment is posted (fac-349).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub declined: bool,
    /// Closed when an unsent failed round's submission was withdrawn, not verified fixed (fac-651).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withdrawn: bool,
    /// The latest successful read, retained when a later round cannot read the thread (fac-448).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<Thread>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_error: Option<String>,
    /// The reviewer's one answer and its drop, each tied to the outward reply (fac-448).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<ActionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<ActionId>,
}

/// Each open finding's thread read for a round, including failures (fac-448).
pub type Threads = BTreeMap<u32, Result<Thread, String>>;

/// Reads `Threads` where its holder sits inside a tagged enum: serde buffers that content and
/// hands a map's keys back as strings, which a `u32` key refuses.
pub fn threads_in<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Threads, D::Error> {
    let by_text = BTreeMap::<String, Result<Thread, String>>::deserialize(d)?;
    by_text.into_iter().map(|(n, t)| Ok((n.parse().map_err(serde::de::Error::custom)?, t))).collect()
}

/// A finding's thread as Bitbucket returned it, excluding its root author's replies and resolutions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub replies: Vec<ThreadReply>,
    pub resolved: Option<ThreadResolution>,
}

/// A colleague's reply, with its comment id, author and creation time as Bitbucket gave them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadReply {
    pub id: u64,
    pub by: String,
    pub at: String,
    pub text: String,
}

/// A colleague's resolution of the thread, which does not resolve the finding (fac-448).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadResolution {
    pub by: String,
    pub at: String,
}

/// A finished PR run's head, integrated base and clean/failed status. A conflict or failed run
/// blocks approval merging (fac-111).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub head: Sha,
    pub on: Sha,
    pub clean: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

/// One round of a review card's review, run by its one foreman.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub n: u32,
    /// The PR source head this round reads, before integrating its destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Sha>,
    /// The commit its reviewers read, and the base its diff is against.
    pub commit: Sha,
    pub base: Sha,
    /// Existing tests and assertions the checks flagged against this round's base.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_changes: Vec<String>,
    /// Files reviewed as `commit` has them: an entry's source-head changes, otherwise unread, unapproved content.
    /// Later rounds omit paths matching their own base in both this candidate and the last round with a verdict.
    pub files: Contents,
    /// Files outside this round's changes, approved with its pass; their open findings still need verification.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub carried: Contents,
    /// The seq of its request, once its clone is made: a verdict of a turn that never read it is stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// What became of a failed round's proofs, copied into its fixer's clone; none for a round recorded
    /// before the copy was, or one with no fixer (fac-377).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proofs: Option<Proofs>,
    /// Our card's branch head this round reads, as submitted, before its base is merged in (fac-638).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted: Option<Sha>,
    /// The numbers of the findings its failed verdict named, those raised again included (fac-638); empty on a
    /// round an older build recorded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named: Vec<u32>,
    /// Each reviewer's latest verdict and reported findings in this round (fac-651).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reviewers: BTreeMap<String, ReviewerVerdict>,
}

/// One reviewer's latest verdict and reported findings in a round (fac-651).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewerVerdict {
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<ReportedFinding>,
}

/// One finding a reviewer reported in its verdict, without its proof (fac-651).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedFinding {
    pub file: String,
    pub line: i64,
    pub summary: String,
}

/// The result of copying a failed round's own proofs folder to its fixer's clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Proofs {
    /// Not yet copied: the failed verdict's report waits for the copy; `said` is the verdict's own words,
    /// told when it named no findings.
    Pending { said: String },
    /// Copied: this many files.
    Copied { files: u32 },
    /// Its folder was missing or held no file: none were saved.
    Empty,
    /// The copy failed, for this reason: whether any were saved is unknown.
    Failed { why: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Passed,
    Failed,
    /// Its content changed before its verdict: a later round reviews what lands now.
    Superseded,
}

impl Round {
    /// Everything its pass approves: the files it reviews and those it carries.
    pub fn contents(&self) -> Contents {
        self.carried.iter().chain(&self.files).map(|(f, c)| (f.clone(), c.clone())).collect()
    }
}

impl Review {
    /// The latest round that returned a verdict, including a failed review but excluding superseded work.
    pub fn last_read(&self) -> Option<&Round> {
        self.rounds.iter().rev().find(|r| matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)))
    }

    /// The round under way: its clone being made, or asked, with no verdict yet.
    pub fn open(&self) -> Option<&Round> {
        self.rounds.last().filter(|r| r.verdict.is_none())
    }

    pub fn open_mut(&mut self) -> Option<&mut Round> {
        self.rounds.last_mut().filter(|r| r.verdict.is_none())
    }

    /// Whether a failed round's proofs are still to be copied: its report, and the next round, wait for it (fac-377).
    pub fn proofs_pending(&self) -> bool {
        self.rounds.iter().any(|r| matches!(r.proofs, Some(Proofs::Pending { .. })))
    }

    /// What of `contents` no passed round approved as it is.
    pub fn uncovered(&self, contents: &Contents) -> Contents {
        contents
            .iter()
            .filter(|(f, c)| self.approved.get(*f) != Some(*c))
            .map(|(f, c)| (f.clone(), c.clone()))
            .collect()
    }

    /// Whether `contents` may land: a round passed, and every file of it is approved as it is.
    pub fn covers(&self, contents: &Contents) -> bool {
        self.current().any(|r| r.verdict == Some(Verdict::Passed)) && self.uncovered(contents).is_empty()
    }

    /// The rounds of the plan in force: all of them, or those from a plan agreed again after `brief` (fac-638).
    pub fn current(&self) -> impl DoubleEndedIterator<Item = &Round> {
        self.rounds.iter().filter(|r| self.afresh.is_none_or(|from| r.n >= from))
    }

    /// The latest round read since the plan in force, when it failed and every finding its verdict named is
    /// declined, none open (fac-638): what it read counts as passed.
    pub fn declined(&self) -> Option<&Round> {
        let r = self.current().rev().find(|r| matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)))?;
        let declined = |n: &u32| self.raised.iter().any(|x| x.n == *n && x.resolved && x.declined);
        let all = r.verdict == Some(Verdict::Failed) && !r.named.is_empty() && r.named.iter().all(declined);
        (all && self.open_findings().next().is_none()).then_some(r)
    }

    /// Whether `contents` needs no round: covered, and no finding waits for its raiser to verify its
    /// fix, a revert's too (fac-111).
    pub fn settled(&self, contents: &Contents) -> bool {
        self.covers(contents) && self.open_findings().next().is_none()
    }

    /// Each file's content as the latest round with its own verdict read it, which a fix round leaves out
    /// unchanged (fac-111); not content a resolved finding was raised on: its resolve verified other content.
    pub fn read(&self) -> Contents {
        let mut out = Contents::new();
        let done = self.current().filter(|r| matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)));
        for r in done {
            out.extend(r.carried.iter().chain(&r.files).map(|(f, c)| (f.clone(), c.clone())));
        }
        let raised_on = |x: &Raised| self.rounds.iter().find(|r| r.n == x.round).map(Round::contents);
        let rejected: Vec<(String, String)> = (self.raised.iter().filter(|x| x.resolved))
            .filter_map(|x| Some((x.file.clone(), raised_on(x)?.get(&x.file)?.clone())))
            .collect();
        out.retain(|f, c| !rejected.iter().any(|(g, d)| g == f && d == c));
        out
    }

    /// The findings not yet verified fixed by whoever raised them.
    pub fn open_findings(&self) -> impl Iterator<Item = &Raised> {
        self.raised.iter().filter(|r| !r.resolved)
    }
}

/// Where a raised review finding stands: a drop outranks a decline, which outranks a plain resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingState {
    Open,
    Resolved,
    Declined,
    Dropped,
}

/// One raised review finding as an overview round carries it, with its proof (fac-674, fac-698).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingView {
    pub n: u32,
    pub round: u32,
    pub by: String,
    pub file: String,
    pub line: i64,
    pub summary: String,
    pub state: FindingState,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proof: String,
}

/// One finding in a pending triage: its 1-based position in the round (`<i>-` proofs) and its number (`#n`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageItem {
    pub pos: u32,
    pub n: u32,
}

/// A failed review round of our card held for triage when delegation is off or on a delegated stalemate, or for direct foreman dispatch when delegated (fac-680, fac-689, fac-694).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTriage {
    pub ask: crate::AskId,
    pub review: CardId,
    pub round: u32,
    pub commit: Sha,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Sha>,
    pub items: Vec<TriageItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_failure: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_verification: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Sha>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stalemate: bool,
}

/// A folded internal review card's seats and current round for its parent card's room (fac-680).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldedReview {
    pub card: CardId,
    pub round: u32,
    /// Its foreman, the parent of its reviewers, with its own state (fac-696).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreman: Option<crate::view::AgentRow>,
    /// Its reviewers, as spawned.
    pub reviewers: Vec<crate::view::AgentRow>,
    /// Its other seats, its foreman's DM among them: no reviewer, nor its foreman (fac-696).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<crate::view::AgentRow>,
}

/// One review round under the current plan, with the findings its verdict named (fac-680).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundView {
    pub n: u32,
    pub commit: String,
    pub verdict: String,
    #[serde(default)]
    pub findings: Vec<FindingView>,
}

/// One finding in an open triage ask inside the Card Room, with its proof (fac-680).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageFindingView {
    pub n: u32,
    pub by: String,
    pub file: String,
    pub line: i64,
    pub summary: String,
    pub proof: String,
}

/// Open failed-round triage data attached to the card's room view (fac-680, fac-694).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriageView {
    pub ask: crate::AskId,
    pub review: CardId,
    pub round: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stalemate: bool,
    pub findings: Vec<TriageFindingView>,
}
