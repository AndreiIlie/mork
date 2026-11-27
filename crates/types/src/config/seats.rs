use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Config;

/// A foreman's child choices by role and review kind; an absent or empty list grants no selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Picks {
    #[serde(default)]
    pub planner: Vec<PickModel>,
    #[serde(default)]
    pub foreman: ForemanPicks,
    #[serde(default, rename = "review-foreman")]
    pub review_foreman: Vec<PickModel>,
    #[serde(default)]
    pub baseline: Vec<PickModel>,
    #[serde(default)]
    pub pass: Vec<PickModel>,
    #[serde(default)]
    pub researcher: Vec<PickModel>,
    #[serde(default)]
    pub reviewer: ReviewPicks,
}

impl Picks {
    /// The role's explicit permissions for its review kind, independent of its defaults.
    pub fn for_role(&self, role: &str, review: Option<ReviewKind>) -> &[PickModel] {
        match (role, review) {
            ("researcher", _) => &self.researcher,
            ("reviewer", Some(ReviewKind::Inside)) => &self.reviewer.inside,
            ("reviewer", Some(ReviewKind::Outside)) => &self.reviewer.outside,
            _ => &[],
        }
    }
}

/// A reviewer's explicit permissions for each review kind, with no permission inherited between kinds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPicks {
    #[serde(default)]
    pub inside: Vec<PickModel>,
    #[serde(default)]
    pub outside: Vec<PickModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForemanPicks {
    #[serde(default)]
    pub work: Vec<PickModel>,
    #[serde(default)]
    pub report: Vec<PickModel>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    #[default]
    Fixed,
    Shadow,
    Auto,
}

/// A configured child model, independent of any seat's defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickModel {
    pub cli: Cli,
    pub model: String,
    pub label: String,
    pub efforts: Vec<Effort>,
}

/// A phase's defaults over the seat's common CLI, model and effort.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOverride {
    pub routing: Option<RoutingMode>,
    pub cli: Option<Cli>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

/// `[seats.<name>]`: any key of 3.1's table, overriding its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatOverride {
    pub routing: Option<RoutingMode>,
    pub cli: Option<Cli>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub work: Option<ModelOverride>,
    pub report: Option<ModelOverride>,
    pub inside: Option<ModelOverride>,
    pub outside: Option<ModelOverride>,
    pub tools: Option<Vec<String>>,
    pub web: Option<bool>,
    pub credentials: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    /// Claude Code's advisor model (fac-5): `advisorModel` in the turn's settings. Claude seats only.
    pub advisor: Option<String>,
    /// Whether its turns run in a wall (fac-25).
    pub wall: Option<Wall>,
}

impl SeatOverride {
    /// The override for this phase or review classification.
    pub fn at(&self, at: Standing) -> Option<&ModelOverride> {
        match at.review {
            Some(ReviewKind::Inside) => self.inside.as_ref(),
            Some(ReviewKind::Outside) => self.outside.as_ref(),
            None => match at.phase {
                SeatPhase::Spec => None,
                SeatPhase::Work => self.work.as_ref(),
                SeatPhase::Report => self.report.as_ref(),
            },
        }
    }
}

/// A seat's wall (fac-25): its turns walled, or run with none, its deny list in Claude's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wall {
    Full,
    None,
}

/// The CLIs a seat runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Cli {
    Claude,
    Codex,
    Agy,
}

/// An effort (3.1.5): the levels the CLIs know; [`Cli::takes`] names each model's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// Efforts a foreman may choose for its child.
    pub const PICKED: [Self; 2] = [Self::High, Self::Xhigh];

    /// Every supported level, lowest first.
    pub const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max];

    /// The levels available for explicit selections outside the operator's own pick.
    /// Configured seats and auto pass pairs may use a supported `max` effort.
    pub const OFFERED: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Xhigh];

    /// A level by its word.
    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == word)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

impl Cli {
    /// A CLI by its word, as a job's command names it and `pick --cli` does (fac-96).
    pub fn parse(word: &str) -> Option<Self> {
        [Self::Claude, Self::Codex, Self::Agy].into_iter().find(|c| c.as_str() == word)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
        }
    }

    /// The efforts `model` on this CLI runs at, in order (fac-661).
    pub fn efforts(self, model: &str) -> &'static [Effort] {
        match self {
            Self::Agy if model == OPUS => &[Effort::Low, Effort::Medium, Effort::High],
            Self::Agy => &Effort::OFFERED,
            Self::Claude | Self::Codex => &Effort::ALL,
        }
    }

    /// Whether `model` on this CLI runs at `effort`: agy's Opus takes low, medium and high; other agy models take no `max`.
    pub fn takes(self, model: &str, effort: Effort) -> bool {
        self.efforts(model).contains(&effort)
    }
}

/// A seat, its defaults merged with the config's overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub name: String,
    pub cli: Cli,
    pub model: String,
    pub effort: Effort,
    pub label: String,
    /// The seat's own `tools`, when its config sets them; else every tool of its CLI but
    /// `CLAUDE_BLOCKED` (fac-127).
    pub tools: Option<Vec<String>>,
    pub web: bool,
    pub credentials: Option<String>,
    pub env: BTreeMap<String, String>,
    /// The advisor model a Claude seat consults mid-turn (fac-5), if any.
    pub advisor: Option<String>,
    pub wall: Wall,
}

/// The tools a Claude seat never gets, by name: they wait on someone, outlive the turn or leave its
/// folder, which a headless engine can't follow (fac-127).
pub const CLAUDE_BLOCKED: &[&str] = &[
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "CronCreate",
    "CronDelete",
    "CronList",
    "ScheduleWakeup",
    "Monitor",
    "RemoteTrigger",
    "Workflow",
    "PushNotification",
    "EnterWorktree",
    "ExitWorktree",
];

/// The roster's defaults, the foreman's the unwalled report fallback; `echo` is the tests' seat (fac-558, 1.10.4).
pub const SEAT_DEFAULTS: &[(&str, Cli, &str, &str, Effort)] = &[
    ("planner", Cli::Claude, OPUS, "opus 5.5", Effort::Xhigh),
    ("foreman", Cli::Claude, OPUS, "opus 5.5", Effort::Xhigh),
    ("review-foreman", Cli::Codex, SOL, "sol", Effort::Xhigh),
    ("reviewer", Cli::Codex, SOL, "sol", Effort::Xhigh),
    ("researcher", Cli::Codex, SOL, "sol", Effort::Xhigh),
    ("baseline", Cli::Claude, SONNET, "sonnet", Effort::High),
    ("echo", Cli::Claude, SONNET, "sonnet", Effort::Low),
];

type Row = (&'static str, SeatPhase, Option<ReviewKind>, Cli, &'static str, &'static str, Effort);

/// Fallback phase defaults, independent of foreman permissions and implementation authorship.
pub const ROSTER: &[Row] = &[
    ("foreman", SeatPhase::Work, None, Cli::Codex, SOL, "sol", Effort::Xhigh),
    ("foreman", SeatPhase::Report, None, Cli::Codex, SOL, "sol", Effort::Xhigh),
];

/// Opus 5.5's model id.
pub const OPUS: &str = "claude-opus-5-5";
/// GPT-6.2 Sol's model id.
pub const SOL: &str = "gpt-6.2-sol";
pub(super) const SONNET: &str = "claude-sonnet-5-5";
pub(super) const ASTRA: &str = "gpt-6.2-astra";
/// Gemini 4 Neon's model id; agy takes `<model>-<effort>`.
pub const NEON: &str = "gemini-4-neon";
/// agy's third-party quota bucket (fac-661); its Gemini Models bucket stays `account`.
pub const AGY_3P_BUCKET: &str = "3p";

/// The agy quota bucket `model` draws from: Gemini models use `account`, all others `3p` (fac-661).
pub fn agy_bucket(model: &str) -> &'static str {
    if model.starts_with("gemini-") { "account" } else { AGY_3P_BUCKET }
}

/// Whether only the operator's own pick may explicitly select this model.
/// Configured seats, auto pass pairs and launched sessions can use Fable or Astra.
pub fn barred(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("fable") || m.contains("astra")
}

/// Standard API dollars per million tokens, in thousandths: uncached input, cache read, 30m write, output.
pub const CODEX_PRICES: &[(&str, [u64; 4])] = &[
    (SOL, [2000, 100, 2500, 10000]),
    (ASTRA, [8000, 400, 10000, 40000]),
    ("gpt-6.1-sol", [2000, 100, 2500, 10000]),
    ("gpt-6.1-terra", [1000, 50, 1250, 5000]),
    ("gpt-6-astra", [10000, 1000, 12500, 50000]),
    ("gpt-6-luna", [100, 10, 125, 500]),
];

/// agy models' standard API dollars per million tokens, in thousandths and as `CODEX_PRICES`: thinking is output.
pub const AGY_PRICES: &[(&str, [u64; 4])] = &[(NEON, [3000, 150, 0, 15000]), (OPUS, [4000, 200, 0, 20000])];

/// A seat's roster phase, including the retired `spec` phase of a saved choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeatPhase {
    Spec,
    Report,
    Work,
}

/// Whether a review is linked to our work or reviews outside work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewKind {
    Inside,
    Outside,
}

/// A seat's phase and, for a review seat, the work's classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub phase: SeatPhase,
    pub review: Option<ReviewKind>,
}

impl Standing {
    /// At its work, outside a review.
    pub const WORK: Self = Self { phase: SeatPhase::Work, review: None };

    /// Every phase and review policy that may supply configured models.
    pub const ALL: [Self; 4] = [
        Self::WORK,
        Self { phase: SeatPhase::Report, review: None },
        Self { phase: SeatPhase::Work, review: Some(ReviewKind::Inside) },
        Self { phase: SeatPhase::Work, review: Some(ReviewKind::Outside) },
    ];
}

/// Whether a review seat's default at this standing moved from Claude to Codex (fac-558): outside reviews were
/// already on Codex.
pub fn moved_to_codex(name: &str, at: Standing) -> bool {
    matches!(name, "reviewer" | "review-foreman") && at.review != Some(ReviewKind::Outside)
}

/// The CLI a configured model runs on, where its id tells.
pub fn cli_of_model(model: &str) -> Option<Cli> {
    MODEL_CATALOG
        .iter()
        .chain(OPERATOR_MODELS)
        .find(|c| c.1 == model)
        .map(|c| c.0)
        .or_else(|| CODEX_PRICES.iter().any(|(id, _)| *id == model).then_some(Cli::Codex))
        .or_else(|| model.starts_with("claude-").then_some(Cli::Claude))
}

type SeatDefault = (Cli, &'static str, &'static str, Effort);

// A seat's default before its configured model. An unwalled review seat naming neither CLI nor model keeps Claude's
// deny rules where its default moved; a CLI named alone runs a model of its own.
fn default_in(name: &str, at: Standing, o: &SeatOverride, on: Option<Cli>, chosen: bool) -> Option<SeatDefault> {
    let base = SEAT_DEFAULTS.iter().find(|s| s.0 == name)?;
    let row = ROSTER.iter().find(|r| r.0 == name && r.1 == at.phase && r.2.is_none_or(|t| at.review == Some(t)));
    // The report fallback must preserve Claude's unwalled deny list and its model when only its CLI is named.
    let row =
        row.filter(|r| r.1 != SeatPhase::Report || (o.wall != Some(Wall::None) && (on != Some(Cli::Claude) || chosen)));
    let (cli, model, label, effort) = row.map_or((base.1, base.2, base.3, base.4), |r| (r.3, r.4, r.5, r.6));
    let unwalled = o.wall == Some(Wall::None) && on.is_none() && !chosen;
    let cli_alone = on.filter(|c| *c != cli && !chosen);
    Some(match (unwalled && moved_to_codex(name, at), cli_alone) {
        (true, _) | (_, Some(Cli::Claude)) => (Cli::Claude, OPUS, "opus 5.5", effort),
        (_, Some(Cli::Codex)) => (Cli::Codex, SOL, "sol", effort),
        (_, Some(Cli::Agy)) => (Cli::Agy, NEON, "Gemini 4 Neon", effort),
        _ => (cli, model, label, effort),
    })
}

/// Known models offered alongside configured models, without defining a foreman's permissions.
pub const MODEL_CATALOG: &[(Cli, &str, &str)] = &[
    (Cli::Claude, SONNET, "sonnet"),
    (Cli::Claude, OPUS, "opus 5.5"),
    (Cli::Claude, "claude-haiku-5-5", "haiku"),
    (Cli::Codex, "gpt-6.1-terra", "terra"),
    (Cli::Codex, SOL, "sol"),
    (Cli::Agy, NEON, "Gemini 4 Neon"),
    (Cli::Agy, OPUS, "opus 5.5"),
];

/// Additional models offered to the operator's own pick alone (fac-441, fac-615), and the labels configured seats take.
pub const OPERATOR_MODELS: &[(Cli, &str, &str)] =
    &[(Cli::Claude, "claude-fable-5-2", "fable 5.2"), (Cli::Codex, ASTRA, "astra"), (Cli::Codex, "gpt-6-luna", "luna")];

/// The seat name of a card's foreman, using the review policies on a review card.
pub fn foreman_seat(review: bool) -> &'static str {
    if review { "review-foreman" } else { "foreman" }
}

impl Config {
    /// A seat by name at its work: its roster row with `[seats.<name>]` over it.
    pub fn seat(&self, name: &str) -> Option<Seat> {
        self.seat_in(name, Standing::WORK)
    }

    /// A seat's phase or review defaults, then common and phase-specific configuration overrides.
    pub fn seat_in(&self, name: &str, at: Standing) -> Option<Seat> {
        let o = self.seats.get(name).cloned().unwrap_or_default();
        let phase = o.at(at);
        let chosen = phase.and_then(|p| p.model.as_ref()).or(o.model.as_ref());
        let on = phase.and_then(|p| p.cli).or(o.cli);
        let (cli, model, label, effort) = default_in(name, at, &o, on, chosen.is_some())?;
        let catalog = || MODEL_CATALOG.iter().chain(OPERATOR_MODELS);
        // A configured Fable row runs on Claude whatever the seat's default CLI; one its id can't place, an alias such
        // as `fable`, runs where it did before a review seat's default moved, keeping Claude's deny rules (fac-558).
        let unplaced = if chosen.is_some() && moved_to_codex(name, at) { Cli::Claude } else { cli };
        let cli = on.or_else(|| cli_of_model(chosen?)).unwrap_or(unplaced);
        let model = chosen.cloned().unwrap_or_else(|| model.to_owned());
        // An inherited default `xhigh` falls to `high` on a model that lacks it; an explicit effort keeps its hold.
        let effort = phase
            .and_then(|p| p.effort)
            .or(o.effort)
            .unwrap_or_else(|| if cli.takes(&model, effort) { effort } else { Effort::High });
        // A default's label names the default's model: a configured model is labelled as it runs (fac-331).
        let label = chosen.map_or_else(
            || label.to_owned(),
            |m| catalog().find(|c| c.0 == cli && c.1 == m).map_or_else(|| m.clone(), |c| c.2.to_owned()),
        );
        Some(Seat {
            name: name.to_owned(),
            cli,
            model,
            effort,
            label,
            tools: o.tools,
            web: o.web.unwrap_or(true),
            credentials: o.credentials,
            env: o.env.unwrap_or_default(),
            advisor: o.advisor,
            wall: o.wall.unwrap_or(Wall::Full),
        })
    }
}
