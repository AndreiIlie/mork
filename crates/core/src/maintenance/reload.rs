//! Whether a reload may go ahead (1.8.6.1): refused before anything changes.

use std::fmt;

/// What the engine found about the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The build exists, with its manifest's ranges.
    pub manifest: Option<Manifest>,
    /// Its commit is on the factory repo's master.
    pub on_master: bool,
}

/// The parts of a build's manifest a reload checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub state_oldest: u32,
    pub state_newest: u32,
    pub job_formats: Vec<u32>,
    pub reply_schemas: Vec<u32>,
}

/// What the unsettled jobs need a build to read (2.3.6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unsettled {
    pub formats: Vec<u32>,
    pub reply_schemas: Vec<u32>,
}

/// Why a reload (or a pin, for its first two) is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NoBuild {
        short: String,
    },
    NotOnMaster {
        short: String,
    },
    /// The target can't read the state's version: a rollback too far, to which the engine adds
    /// the state's backups (1.8.8).
    StateVersion {
        short: String,
        oldest: u32,
        newest: u32,
        state: u32,
    },
    JobFormat {
        short: String,
        format: u32,
    },
    ReplySchema {
        short: String,
        schema: u32,
    },
    UnderWay,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBuild { short } => write!(f, "there is no build of {short}"),
            Self::NotOnMaster { short } => {
                write!(f, "{short} isn't on the factory repo's master: a pin is a commit Andrei merged")
            }
            Self::StateVersion { short, oldest, newest, state } => {
                write!(f, "{short} reads state versions {oldest} to {newest}, and the state is at {state}")
            }
            Self::JobFormat { short, format } => {
                write!(f, "an unsettled job has format {format}, which {short} doesn't read")
            }
            Self::ReplySchema { short, schema } => {
                write!(f, "an unsettled job has reply schema {schema}, which {short} doesn't read")
            }
            Self::UnderWay => write!(f, "another reload is under way"),
        }
    }
}

/// Why a reload (or a pin, for its first two) is refused, if it is.
pub fn refusal(
    commit: &str,
    t: &Target,
    state_version: u32,
    unsettled: &Unsettled,
    under_way: bool,
) -> Option<Refusal> {
    let short: String = commit.chars().take(7).collect();
    let Some(m) = &t.manifest else { return Some(Refusal::NoBuild { short }) };
    if !t.on_master {
        return Some(Refusal::NotOnMaster { short });
    }
    if state_version < m.state_oldest || state_version > m.state_newest {
        return Some(Refusal::StateVersion {
            short,
            oldest: m.state_oldest,
            newest: m.state_newest,
            state: state_version,
        });
    }
    if let Some(f) = unsettled.formats.iter().find(|f| !m.job_formats.contains(f)) {
        return Some(Refusal::JobFormat { short, format: *f });
    }
    if let Some(s) = unsettled.reply_schemas.iter().find(|s| !m.reply_schemas.contains(s)) {
        return Some(Refusal::ReplySchema { short, schema: *s });
    }
    if under_way {
        return Some(Refusal::UnderWay);
    }
    None
}
