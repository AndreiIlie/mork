use mork_types::config::{ForgeConfig, ProjectConfig};
use mork_types::review::Threads;
use mork_types::work::{ActionFailure, OpenPr, PrSeen};

use crate::bitbucket;

pub(crate) type Result<T> = std::result::Result<T, ActionFailure>;
#[derive(Debug, Clone, Default)]
pub(crate) struct Readers {
    fulls: bitbucket::Fulls,
}

pub(crate) enum Identity {
    Known(Option<String>),
    Unknown,
}

pub(crate) struct Pr {
    pub id: Option<u64>,
    pub title: Option<String>,
    pub description: String,
    pub open: bool,
    pub state: String,
    pub branch: Option<String>,
    pub dest: Option<String>,
}

pub(crate) struct Comment {
    pub id: Option<u64>,
    pub parent: Option<u64>,
    pub body: Option<String>,
    pub inline: Option<Anchor>,
}

pub(crate) struct Anchor {
    pub path: Option<String>,
    pub line: Option<u64>,
}

pub(crate) trait ForgeRead {
    fn identity(&self) -> std::result::Result<Identity, String>;
    fn has_approver(&self) -> bool;
    fn open_prs(&self) -> Result<Vec<OpenPr>>;
    fn pr(&self, id: u64) -> Result<Pr>;
    fn branch_prs(&self, branch: &str) -> Result<Vec<Pr>>;
    fn observe(&self, id: u64) -> Result<PrSeen>;
    fn comments(&self, pr: u64, complete: bool) -> Result<Vec<Comment>>;
    fn threads(&self, pr: u64, ids: &[(u32, u64)]) -> Threads;
}

pub(crate) trait ForgeWrite {
    fn publish(&self, branch: &str, base: &str, text: (&str, &str), pr: Option<u64>) -> Result<u64>;
    fn comment(&self, pr: u64, file: &str, line: u64, body: &str) -> Result<Comment>;
    fn reply(&self, pr: u64, parent: u64, body: &str) -> Result<Option<u64>>;
    fn resolve(&self, pr: u64, comment: u64) -> Result<bool>;
    fn merge(&self, pr: u64) -> Result<String>;
}

fn local() -> ActionFailure {
    ActionFailure::Failed { words: "the project's forge is local".into() }
}

impl Readers {
    pub(crate) fn read(&self, config: &ForgeConfig) -> Result<Box<dyn ForgeRead>> {
        match config {
            ForgeConfig::Local { .. } => Err(local()),
            ForgeConfig::Bitbucket { workspace, slug, review_tag, .. } => {
                Ok(Box::new(bitbucket::Reader::new(workspace, slug, review_tag.clone(), self.fulls.clone())?))
            }
        }
    }
}

pub(crate) fn write(config: &ForgeConfig) -> Result<Box<dyn ForgeWrite>> {
    match config {
        ForgeConfig::Local { .. } => Err(local()),
        ForgeConfig::Bitbucket { workspace, slug, .. } => Ok(Box::new(bitbucket::Client::new(workspace, slug)?)),
    }
}

pub(crate) fn source(project: &ProjectConfig) -> std::result::Result<(String, Vec<(String, String)>), String> {
    match &project.forge {
        ForgeConfig::Local { .. } => Ok((project.repo.clone(), Vec::new())),
        ForgeConfig::Bitbucket { workspace, slug, .. } => bitbucket::source(workspace, slug),
    }
}

pub(crate) enum Credential {
    Bitbucket(String),
}

impl Credential {
    pub fn read(config: &ForgeConfig) -> Result<Self> {
        match config {
            ForgeConfig::Local { .. } => Err(local()),
            ForgeConfig::Bitbucket { .. } => bitbucket::token_text().map(Self::Bitbucket),
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Bitbucket(text) => text,
        }
    }

    pub fn pinned<T>(&self, call: impl FnOnce() -> T) -> T {
        match self {
            Self::Bitbucket(text) => bitbucket::pinned(text.clone(), call),
        }
    }
}
