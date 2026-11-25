//! A train entry's landing (fac-68, fac-111): its steps, and the candidate it makes and runs.

use serde::{Deserialize, Serialize};

/// An entry's ordered candidate, integration, checks, review, suite and landing steps.
/// PR merging rechecks the PR before the call (4.11.2, fac-10, fac-68, fac-111).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case")]
pub enum Landing {
    /// Admitted at this time, when its gates passed, waiting for the train's window.
    Queued { at: i64 },
    /// At the train's front with no candidate: the base fetched first.
    Fetch { at: i64 },
    /// A PR merged outside the factory, its merge commit not yet in the mirror: fetched, then its
    /// PR read again, whatever the fetch says of the head (M5.3).
    HandMerged,
    /// Its candidate being made in its train clone (fac-68).
    Prepare(Candidate),
    /// Its candidate made, under its checks in its train clone, or, once `checked`, its suite, its
    /// review round running beside it (fac-375).
    Test(Candidate),
    /// Its suite ended, its review round still under way (fac-111, fac-375).
    Review(Candidate),
    /// Verified: in the merge queue, landing at its turn (fac-375).
    Passed(Candidate),
    /// Tested on Bitbucket: `commit` pushed, its PR opened. An older build's lands the card's head.
    /// `verified`: its round and suite passed on `commit` and `base` before it was published (fac-375).
    Published {
        tree: Option<String>,
        base: crate::ids::Sha,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<crate::ids::Sha>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        verified: bool,
    },
    /// A PR published, waiting out of the train for Andrei's approval, never merged before it (fac-98, fac-111).
    /// An older build's, not `verified`, merges on its PR entry's run instead (fac-375).
    Approval {
        tree: Option<String>,
        base: crate::ids::Sha,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<crate::ids::Sha>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        verified: bool,
    },
    Merge {
        tree: Option<String>,
        base: crate::ids::Sha,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<crate::ids::Sha>,
    },
}

/// A failed review round held on a candidate until its suite ends too (fac-375, fac-680).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HeldRound {
    /// A failed round with no findings: its fix request text goes straight to the foreman.
    Text { text: String },
    /// A failed round with findings: held until the suite ends, then triaged or sent straight to the foreman when delegated.
    Triage(crate::review::PendingTriage),
}

/// A card's candidate (fac-68, fac-375): its head merged onto `on` in a disposable clone of its own,
/// `trains/<card>-<clone>`, verified there and landed by the merge queue; the card's branch never sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// The time it was admitted; once verified, the time it entered the merge queue, its place there.
    pub at: i64,
    pub on: crate::ids::Sha,
    /// Its clone's tag: the time it was asked for.
    pub clone: i64,
    /// The commit made in its clone, and its tree; none while it is made, or when it conflicted.
    pub commit: Option<crate::ids::Sha>,
    pub tree: Option<String>,
    /// What its checks or its suite found, kept until its round ends too; none for a conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// What the commit makes of each file it changes past `on` (fac-95).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub contents: crate::review::Contents,
    /// A PR's changes since its last reviewed source head, as the integrated candidate has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_changes: Option<crate::review::Contents>,
    /// Its checks ran: its run under way is its suite (fac-111).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    /// The failing tests its suite named, for the comment of a failed PR run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failing: Vec<crate::work::Failing>,
    /// Its failed review round held until its suite ends too (fac-375, fac-680).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<HeldRound>,
    /// Made again at the merge queue's front on a moved base: its checks and suite run again, no round (fac-375).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue: bool,
    /// Our approved PR's published head, verified again on its destination's new tip: once passed, it merges
    /// while the approval stands, nothing pushed (fac-375).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<crate::ids::Sha>,
}
