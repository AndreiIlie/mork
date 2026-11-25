//! Outward actions (2.5): recorded before they are done, each with its natural key.

use serde::{Deserialize, Serialize};

use crate::ids::{CardId, Sha};

/// What an action does, with what it needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionWhat {
    /// Fetch origin into a project's mirror.
    Fetch { project: crate::ProjectId, at: i64 },
    /// Take a card's commits: its bundle verified and fetched into the mirror.
    Take { card: CardId, head: Sha, job: crate::ids::JobId },
    /// An older build's take of a passed join's commit (4.11.2.3, M3.19): none is taken now (fac-308).
    TakeJoin { card: CardId, head: Sha, tip: Sha, job: crate::ids::JobId },
    /// Push a card's branch, never forced.
    Push { card: CardId, branch: String, commit: Sha },
    /// Push the local base's current tip to its backup; `landed` identifies the triggering landing.
    BackupPush { project: crate::ProjectId, base: String, remote: String, landed: Sha },
    /// Open the card's PR in Andrei's name (fac-98), or, the card having one (`pr`), give that exact
    /// PR this title and description, none for a card with no ticket (fac-111, fac-290).
    OpenPr {
        card: CardId,
        project: crate::ProjectId,
        branch: String,
        title: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pr: Option<u64>,
    },
    /// Post a finding as an inline comment on a colleague's PR.
    Comment {
        card: CardId,
        pr: u64,
        file: String,
        line: u64,
        body: String,
        /// The repository (`workspace/slug`): a PR's number is its repository's (M3.18).
        #[serde(default)]
        repo: String,
    },
    /// Reply under a finding's comment for the head that still leaves it unresolved.
    ReplyComment { card: CardId, pr: u64, comment: u64, commit: Sha, body: String, repo: String },
    /// Resolve a comment a review posted, once its raiser verified its fix (fac-111).
    ResolveComment {
        card: CardId,
        pr: u64,
        comment: u64,
        #[serde(default)]
        repo: String,
    },
    /// Merge a PR through Bitbucket; the merge commit's tree must be `tree` (the joined tree),
    /// or the head's, and its first parent `base` (4.11.2.5).
    Merge {
        card: CardId,
        pr: u64,
        head: Sha,
        tree: Option<String>,
        base: Sha,
        #[serde(default)]
        repo: String,
        /// A colleague's PR's destination, which it merges into: one moved since its review is
        /// checked for the files the PR touches. None for a Factory card, merged into the base.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dest: Option<String>,
    },
    /// Move a local base to a target commit (`/merge`).
    MoveBase { card: CardId, project: crate::ProjectId, base: String, from: Sha, target: Sha },
    /// Write a new board card. `core` decides its id, its file and its whole text when it makes
    /// the action (M3.16); the key is the board, the file and the text's hash (2.5).
    CreateBoardCard {
        project: crate::ProjectId,
        title: String,
        description: String,
        id: Option<CardId>,
        #[serde(default)]
        file: String,
        #[serde(default)]
        text: String,
        /// The card it is written for, when it is one's (M3.23).
        #[serde(default)]
        card: Option<CardId>,
    },
    /// Change a board card's file; the key is the file and the hashes before and after (2.5, M3.16).
    /// Empty `before`, `after` and `text` leave it to the engine, for a file the boards' read lacks.
    ChangeBoardCard {
        project: crate::ProjectId,
        file: String,
        edit: BoardEdit,
        #[serde(default)]
        before: String,
        #[serde(default)]
        after: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        dest: String,
        #[serde(default)]
        card: Option<CardId>,
        /// The carried card whose `merged` status its run's landing writes: held to that run until this write is
        /// settled, wherever its file moves or whatever `merged` is now (fac-563). Never part of the key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lands: Option<CardId>,
    },
    /// Remove a card's clone; a review card closed to reopen on a new head keeps its proofs for
    /// it (fac-276).
    RemoveClone {
        card: CardId,
        run: u32,
        #[serde(default)]
        keep_proofs: bool,
    },
    /// Remove one of a card's train clones, `trains/<card>-<clone>`, its jobs ended (fac-68).
    RemoveTrain { card: CardId, clone: i64 },
    /// A door call (fac-1): a read on a target outside every wall, or the misc door's command.
    Door(Box<crate::door_call::DoorCall>),
}

/// A change to a board card (4.18.3, 4.18.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "edit", rename_all = "kebab-case")]
pub enum BoardEdit {
    /// A dated note at the top of the description.
    Note { who: String, text: String },
    /// Each `Some` replaces its value; `acceptance` adds a criterion per line. Edits saved before
    /// `description` and `note` read them as absent.
    Fields {
        title: Option<String>,
        status: Option<String>,
        priority: Option<String>,
        labels: Option<Vec<String>>,
        parent: Option<String>,
        acceptance: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<Box<BoardNote>>,
    },
    /// Moved to `completed/` (done) or `archive/tasks/` (dropped), with a closing note.
    Close { done: bool, who: String, note: String },
}

/// A dated note's author and words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardNote {
    pub who: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStatus {
    Pending,
    Started,
    Done,
    Held,
    Dropped,
}

/// An action's row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: crate::ActionId,
    pub what: ActionWhat,
    /// Its natural key (2.5.4): two actions with one key are one action.
    pub key: String,
    pub status: ActionStatus,
    /// What it returned, or what was found after a crash.
    pub result: Option<serde_json::Value>,
    /// Why it is held.
    pub words: Option<String>,
    pub attempts: u32,
    pub created_ms: i64,
    pub done_ms: Option<i64>,
    /// Waiting for the card's `resume`: cancelled by a stop (4.13.1).
    pub cancelled: bool,
    /// A door call's checks, ask, approval and what its requester was told (fac-1); none for
    /// any other action. Version 5 added it.
    pub door: Option<Box<crate::door_call::DoorState>>,
    /// A standalone note's or a push's proof that no attempt of it wrote anything outside (fac-318, fac-394): `false`
    /// once one may have, for good; none on any other action, nor on one saved before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unwritten: Option<bool>,
    /// A push or PR opening under way when its card's mock reopened (fac-591): its success is recorded, its failure or
    /// a recovery finding it undone drops it, and nothing retries it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withdrawn: bool,
}

fn short(s: &Sha) -> &str {
    s.as_str()
}

impl ActionWhat {
    /// The key an older build gave a PR opening, the branch's alone (fac-382): none for any other
    /// action. Only the same card's action under it is the same action.
    pub fn legacy_key(&self) -> Option<String> {
        match self {
            Self::OpenPr { project, branch, .. } => Some(format!("open-pr:{project}:{branch}")),
            _ => None,
        }
    }

    /// Its natural key (2.5): two actions with one key are one action.
    pub fn key(&self) -> String {
        match self {
            Self::Fetch { project, at } => format!("fetch:{project}:{at}"),
            Self::Take { card, head, .. } => format!("take:{card}:{}", short(head)),
            Self::TakeJoin { card, head, tip, .. } => format!("take-join:{card}:{}:{}", short(head), short(tip)),
            Self::Push { card, branch, commit } => format!("push:{card}:{branch}:{}", short(commit)),
            Self::BackupPush { project, base, remote, landed } => {
                format!("backup-push:{project}:{base}:{remote}:{}", short(landed))
            }
            Self::OpenPr { card, project, branch, .. } => format!("open-pr:{project}:{branch}:{card}"),
            Self::Comment { pr, file, line, body, repo, .. } => {
                format!("comment:{repo}:{pr}:{file}:{line}:{}", crate::hash::short(body))
            }
            Self::ReplyComment { pr, comment, commit, repo, .. } => {
                format!("reply-comment:{repo}:{pr}:{comment}:{}", short(commit))
            }
            Self::ResolveComment { pr, comment, repo, .. } => format!("resolve-comment:{repo}:{pr}:{comment}"),
            Self::Merge { pr, head, repo, .. } => format!("merge:{repo}:{pr}:{}", short(head)),
            Self::MoveBase { project, base, target, .. } => format!("move-base:{project}:{base}:{}", short(target)),
            Self::CreateBoardCard { project, file, text, .. } if !file.is_empty() => {
                format!("create-card:{project}:{file}:{}", crate::hash::short(text))
            }
            Self::CreateBoardCard { project, title, description, id, .. } => {
                let what = format!("{title}\n{description}\n{}", id.as_deref().unwrap_or(""));
                format!("create-card:{project}:{}", crate::hash::short(&what))
            }
            Self::ChangeBoardCard { project, file, before, after, .. } if !after.is_empty() => {
                format!("change-card:{project}:{file}:{before}:{after}")
            }
            Self::ChangeBoardCard { project, file, edit, .. } => {
                let what = serde_json::to_string(edit).unwrap_or_default();
                format!("change-card:{project}:{file}:{}", crate::hash::short(&what))
            }
            Self::RemoveClone { card, run, keep_proofs: false } => format!("remove-clone:{card}:{run}"),
            Self::RemoveClone { card, run, keep_proofs: true } => format!("remove-clone:{card}:{run}:proofs"),
            Self::RemoveTrain { card, clone } => format!("remove-train:{card}:{clone}"),
            // A lookup key only: the same call is found by comparing its request whole.
            Self::Door(c) => format!("door:{}:{}", c.scope(), crate::hash::short(&c.request())),
        }
    }

    /// A board write whose content `core` decided when it made it (M3.16): its key is its
    /// content's, so one done is the same write, never a second.
    pub fn decided(&self) -> bool {
        match self {
            Self::CreateBoardCard { file, .. } => !file.is_empty(),
            Self::ChangeBoardCard { after, .. } => !after.is_empty(),
            Self::Fetch { .. }
            | Self::Take { .. }
            | Self::TakeJoin { .. }
            | Self::Push { .. }
            | Self::BackupPush { .. }
            | Self::OpenPr { .. }
            | Self::Comment { .. }
            | Self::ReplyComment { .. }
            | Self::ResolveComment { .. }
            | Self::Merge { .. }
            | Self::MoveBase { .. }
            | Self::RemoveClone { .. }
            | Self::RemoveTrain { .. }
            | Self::Door(_) => true,
        }
    }

    /// Whether it calls Bitbucket: held while its token is refused.
    pub fn bitbucket(&self) -> bool {
        matches!(
            self,
            Self::Push { .. }
                | Self::OpenPr { .. }
                | Self::Comment { .. }
                | Self::ReplyComment { .. }
                | Self::ResolveComment { .. }
                | Self::Merge { .. }
        )
    }

    /// The card it acts for, if any.
    pub fn card(&self) -> Option<&CardId> {
        match self {
            Self::Take { card, .. }
            | Self::TakeJoin { card, .. }
            | Self::Push { card, .. }
            | Self::OpenPr { card, .. }
            | Self::Comment { card, .. }
            | Self::ReplyComment { card, .. }
            | Self::ResolveComment { card, .. }
            | Self::Merge { card, .. }
            | Self::MoveBase { card, .. }
            | Self::RemoveClone { card, .. }
            | Self::RemoveTrain { card, .. } => Some(card),
            Self::CreateBoardCard { card, .. } | Self::ChangeBoardCard { card, .. } => card.as_ref(),
            Self::Door(c) => c.card.as_ref(),
            Self::Fetch { .. } | Self::BackupPush { .. } => None,
        }
    }
}
