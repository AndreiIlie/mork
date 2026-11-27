use serde::{Deserialize, Serialize};

use crate::config::{Cli, Effort};

/// Tool invocations observed in a turn, with failed calls grouped by their stream tool name.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCounts {
    pub calls: u64,
    pub failed: u64,
    pub by: std::collections::BTreeMap<String, u64>,
}

/// Four disjoint token counts: input excludes cache reads and writes; output includes reasoning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Tokens {
    /// Adds each count, saturating at its range.
    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
            output: self.output.saturating_add(other.output),
        }
    }

    /// The share since an earlier total; a counter that went back contributes nothing.
    pub fn saturating_sub(self, before: Self) -> Self {
        Self {
            input: self.input.saturating_sub(before.input),
            cache_read: self.cache_read.saturating_sub(before.cache_read),
            cache_write: self.cache_write.saturating_sub(before.cache_write),
            output: self.output.saturating_sub(before.output),
        }
    }
}

/// The CLI, model and effort a turn launched with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub cli: Cli,
    pub model: String,
    pub effort: Effort,
}

impl Selection {
    /// Reads the selection from a saved CLI command, independent of later picks or configuration.
    pub fn from_argv(argv: &[String]) -> Option<Self> {
        let cli = Cli::parse(argv.first()?)?;
        let flag = |name: &str| argv.windows(2).find(|w| w[0] == name).map(|w| w[1].as_str());
        // agy takes its effort in its model's name, as `agy models` lists them (`gemini-4-neon-high`, `claude-opus-5-5-high`).
        if cli == Cli::Agy {
            let (model, effort) = flag("--model")?.rsplit_once('-')?;
            return Some(Self { cli, model: model.to_owned(), effort: Effort::parse(effort)? });
        }
        let setting = |name: &str| argv.windows(2).filter(|w| w[0] == "-c").find_map(|w| w[1].strip_prefix(name));
        let model = flag("--model")
            .or_else(|| flag("-m"))
            .map(str::to_owned)
            .or_else(|| setting("model=").and_then(|v| serde_json::from_str(v).ok()))?;
        let effort = flag("--effort").or_else(|| setting("model_reasoning_effort=")).and_then(Effort::parse)?;
        Some(Self { cli, model, effort })
    }
}
