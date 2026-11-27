//! Usage limits (1.4.8, Claude 10; Codex 7): the latest read of each window.

use serde::{Deserialize, Serialize};

/// The Claude account and organization whose plan owns a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub plan: String,
}

/// One usage-limit window of a CLI's account (1.4.8, Claude 10; Codex 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limit {
    /// `claude`, `codex` or `agy`.
    pub cli: crate::config::Cli,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    /// The bucket: the account's own, or a model's (`seven_day_opus`, `gpt-6-astra`).
    pub bucket: String,
    /// The window: `5h`, `week`, a CLI's own name, or `<n>m`.
    pub window: String,
    pub percent: Option<u32>,
    pub resets_at: Option<i64>,
    pub reached: bool,
    pub seen_ms: i64,
}

impl Limit {
    /// Its key in the state: one latest value per CLI, account, bucket and window.
    pub fn key(&self) -> String {
        match &self.account {
            Some(a) => format!("{}/{}/{}/{}", self.cli.as_str(), a.id, self.bucket, self.window),
            None => format!("{}/{}/{}", self.cli.as_str(), self.bucket, self.window),
        }
    }

    /// The display name of this limit's pool: `agy gemini` or `agy 3p` for agy, else its CLI (fac-661).
    pub fn pool_name(&self) -> String {
        match (self.cli, self.bucket.as_str()) {
            (crate::config::Cli::Agy, "account") => "agy gemini".to_owned(),
            (crate::config::Cli::Agy, b) => format!("agy {b}"),
            (cli, _) => cli.as_str().to_owned(),
        }
    }

    /// The same read, apart from when it was seen: a change worth a journal line (1.4.7.2).
    pub fn same_as(&self, other: &Self) -> bool {
        self.key() == other.key()
            && self.account == other.account
            && self.percent == other.percent
            && self.resets_at == other.resets_at
            && self.reached == other.reached
    }
}

/// One role's dollar share in the rolling burn period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleBurn {
    pub role: String,
    pub cents: u64,
    pub unknown: bool,
}

/// Prompt cache use for a CLI and launch model observed during the burn period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCache {
    pub cli: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit_percent: Option<u32>,
}

/// Tightest weekly pool's estimated cards remaining.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardsLeft {
    pub pool: String,
    pub cards: u64,
}

/// Rolling 7-day burn summary returned in `usage`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentBurn {
    pub since_ms: i64,
    pub total_cents: u64,
    pub unknown: bool,
    pub roles: Vec<RoleBurn>,
    pub ended_cards: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_cents: Option<u64>,
    pub cache_by_model: Vec<ModelCache>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cards_left: Option<CardsLeft>,
    /// The last 14 local days, oldest first, for the Usage charts (fac-696).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub daily: Vec<DayBurn>,
}

/// One local day's spend and landed cards (fac-696).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBurn {
    /// The day's local midnight, in epoch ms, at the screen's current offset.
    pub day_ms: i64,
    pub cents: u64,
    /// A turn of the day had no known price.
    pub unknown: bool,
    /// Cards that landed that day.
    pub landed: u64,
}

#[derive(Debug, Serialize)]
pub struct UsageAnswer {
    pub usage: UsageView,
}

#[derive(Debug, Serialize)]
pub struct UsageView {
    pub by_pool: Vec<String>,
    pub limits: Vec<LimitView>,
    pub contexts: Vec<ContextView>,
    pub recent_burn: RecentBurn,
}

#[derive(Debug, Serialize)]
pub struct ContextView {
    pub agent: crate::AgentKey,
    pub used: u64,
    pub window: Option<u64>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LimitView {
    pub cli: crate::config::Cli,
    pub account: Option<Account>,
    pub bucket: String,
    pub window: String,
    pub name: String,
    pub percent: Option<u32>,
    pub resets_at: Option<i64>,
    pub reached: bool,
    pub seen_ms: i64,
    pub length_ms: Option<i64>,
    pub source: String,
    pub age_ms: i64,
    pub unknown: Option<String>,
}
