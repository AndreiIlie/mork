//! The reply schemas (3.5) follow Codex's stricter rule: every property is required, except
//! Claude's message `images` (fac-451); `additionalProperties: false` holds at every level (1.4.8).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ask::{Choices, Color};
use crate::card::Sketch;
use crate::config::Cli;

/// The reply schemas' version, sent in each reply (3.5) and listed in a build's manifest.
pub const SCHEMA_VERSION: u32 = 7;

/// What a seat does (3.2). `Echo` is the tests' role (1.10.4), known only with `faults`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Planner,
    Foreman,
    Reviewer,
    Researcher,
    /// A visual card's baseline seat (fac-636): the foreman's child reproducing today's pages in the mock's folder.
    Baseline,
    Echo,
}

impl Role {
    pub const ALL: [Self; 6] =
        [Self::Planner, Self::Foreman, Self::Reviewer, Self::Researcher, Self::Baseline, Self::Echo];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Foreman => "foreman",
            Self::Reviewer => "reviewer",
            Self::Researcher => "researcher",
            Self::Baseline => "baseline",
            Self::Echo => "echo",
        }
    }

    /// The role's outcomes (3.5.1).
    pub fn outcomes(self) -> &'static [Outcome] {
        match self {
            Self::Planner => &[Outcome::Working, Outcome::BriefReady, Outcome::Blocked],
            // Its `built` submits the card it runs (fac-94).
            Self::Foreman => &[
                Outcome::Working,
                Outcome::Built,
                Outcome::ReviewPass,
                Outcome::ReviewFail,
                Outcome::ReportReady,
                Outcome::Blocked,
            ],
            Self::Baseline => &[Outcome::Working, Outcome::Built, Outcome::Blocked],
            Self::Reviewer => &[Outcome::ReviewPass, Outcome::ReviewFail, Outcome::Blocked],
            Self::Researcher => &[Outcome::Reported, Outcome::Blocked],
            Self::Echo => &[Outcome::None],
        }
    }
}

/// A stored role, or none for one this build no longer has (fac-119, fac-591).
pub fn known_role<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Role>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
}

/// A reply's outcome (3.5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Working,
    BriefReady,
    ReviewPass,
    ReviewFail,
    ReportReady,
    Blocked,
    Built,
    Reported,
    None,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::BriefReady => "brief-ready",
            Self::ReviewPass => "review-pass",
            Self::ReviewFail => "review-fail",
            Self::ReportReady => "report-ready",
            Self::Blocked => "blocked",
            Self::Built => "built",
            Self::Reported => "reported",
            Self::None => "none",
        }
    }
}

/// The part of a reply addressed to the operator: `ask` flags its message as a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hand {
    pub color: Color,
    pub headline: String,
    pub message: String,
    pub ask: bool,
    /// Marks the ask as a design question (fac-233): shown tagged; auto-given like any `answer` ask when delegated.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub design: bool,
    /// Withdraws the seat's open ask unanswered (fac-271); an empty `ask` alone leaves it open.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withdraw_ask: bool,
    /// Files of the seat's `io/` folder, `$MORK_IO`, shown beside the hand and its ask (fac-278, fac-654).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// The ask's numbered options, in order (fac-696): none when it offers no choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// The option the seat would pick, by its number from 1; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pick: u32,
    /// Why it would pick it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// A recap's sketch (fac-696): what changes, one plain line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    /// A recap's sketch: the code's shape, one plain line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_shape: Vec<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Hand {
    /// Its ask's options and pick, when it offers a choice; a pick naming no option is dropped.
    pub fn choices(&self) -> Option<Choices> {
        if !self.ask || self.options.is_empty() {
            return None;
        }
        let named = usize::try_from(self.pick).is_ok_and(|n| (1..=self.options.len()).contains(&n));
        Some(Choices {
            ask: self.message.trim().to_owned(),
            options: self.options.iter().map(|o| o.trim().to_owned()).collect(),
            pick: named.then_some(self.pick),
            reason: if named { self.reason.trim().to_owned() } else { String::new() },
        })
    }

    /// Its ask as one text, its options and pick in it, for every reader that reads words.
    pub fn ask_words(&self) -> String {
        if self.ask { self.choices().map_or_else(|| self.message.clone(), |c| c.words()) } else { String::new() }
    }

    /// A recap's typed sketch, when it gave one.
    pub fn sketch(&self) -> Option<Sketch> {
        let keep = |v: &[String]| v.iter().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
        let (changes, code_shape) = (keep(&self.changes), keep(&self.code_shape));
        (!changes.is_empty() || !code_shape.is_empty()).then_some(Sketch { changes, code_shape })
    }
}

/// What an agent's message is (fac-18): only a request wants a response. All three wake an
/// agent they reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Request,
    Response,
    Update,
}

impl Intent {
    pub const ALL: [Self; 3] = [Self::Request, Self::Response, Self::Update];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Update => "update",
        }
    }

    /// The verb a prompt's news line says it with.
    pub fn verb(self) -> &'static str {
        match self {
            Self::Request => "asks",
            Self::Response => "answers",
            Self::Update => "notes",
        }
    }
}

/// A message an agent sends (3.4): one that reaches an agent wakes it. A message without its
/// `kind` parses, for the checker to refuse it by name (fac-18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyMessage {
    pub to: String,
    #[serde(default)]
    pub kind: Option<Intent>,
    pub text: String,
    /// Files of the seat's `io/` folder carried with the message into its recipient's turn (fac-278, fac-654).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

/// A finding (section 4): the problem and its proof; the fix is the foreman's
/// (fac-260).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "FindingWire")]
pub struct Finding {
    pub file: String,
    pub line: i64,
    pub summary: String,
    pub proof: String,
}

// A finding from a session or a store before fac-260 still carries `fix`: read, never used.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingWire {
    file: String,
    line: i64,
    summary: String,
    proof: String,
    #[serde(default, rename = "fix")]
    _fix: Option<Value>,
}

impl From<FindingWire> for Finding {
    fn from(w: FindingWire) -> Self {
        Self { file: w.file, line: w.line, summary: w.summary, proof: w.proof }
    }
}

/// A brief's risk tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Money,
    Permissions,
    Schema,
    Security,
}

/// A whole reply (3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub hand: Hand,
    pub messages: Vec<ReplyMessage>,
    pub findings: Vec<Finding>,
    pub risks: Vec<Risk>,
    pub outcome: Outcome,
    pub schema: u32,
}

fn object(props: Value) -> Value {
    let required: Vec<Value> =
        props.as_object().map(|m| m.keys().cloned().map(Value::String).collect()).unwrap_or_default();
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn array_of(item: Value) -> Value {
    json!({"type": "array", "items": item})
}

fn string() -> Value {
    json!({"type": "string"})
}

/// The reply schema of a role (3.5): the same shape for every role, its outcomes its own.
pub fn schema(role: Role) -> Value {
    schema_for(role, false, Cli::Codex)
}

/// The reply schema of a seat: a foreman's child's outcomes are its role's for children.
pub fn schema_for(role: Role, child: bool, cli: Cli) -> Value {
    schema_offering(role, child, cli, role.outcomes())
}

/// `schema_for`, offering the outcomes the seat's turn may end with now (fac-616).
pub fn schema_offering(role: Role, child: bool, cli: Cli, outcomes: &[Outcome]) -> Value {
    if role == Role::Echo {
        return echo_schema();
    }
    let outcomes: Vec<&str> = outcomes.iter().map(|o| o.as_str()).collect();
    let mut hand = json!({
        "color": {"type": "string", "enum": ["grey", "green", "yellow", "red"]},
        "headline": string(),
        "message": string(),
        "ask": {"type": "boolean"},
        "images": array_of(string()),
        "options": array_of(string()),
        "pick": {"type": "integer"},
        "reason": string(),
    });
    // A child's hand reaches its parent (fac-233); only a spec holder recaps (fac-696).
    if matches!(role, Role::Planner | Role::Foreman) && !child {
        hand["design"] = json!({"type": "boolean"});
        hand["changes"] = array_of(string());
        hand["code_shape"] = array_of(string());
    }
    // A seat that isn't a foreman's child can hold an ask open, so it can withdraw it (fac-271).
    if !child {
        hand["withdraw_ask"] = json!({"type": "boolean"});
    }
    let hand = object(hand);
    let kinds: Vec<&str> = Intent::ALL.iter().map(|k| k.as_str()).collect();
    let mut message = object(json!({
        "to": string(), "kind": {"type": "string", "enum": kinds}, "text": string(), "images": array_of(string()),
    }));
    if cli == Cli::Claude {
        message["required"] = json!(["kind", "text", "to"]);
    }
    let finding = object(json!({
        "file": string(), "line": {"type": "integer"}, "summary": string(), "proof": string(),
    }));
    object(json!({
        "hand": hand,
        "messages": array_of(message),
        "findings": array_of(finding),
        "risks": array_of(json!({"type": "string", "enum": ["money", "permissions", "schema", "security"]})),
        "outcome": {"type": "string", "enum": outcomes},
        "schema": {"type": "integer", "enum": [SCHEMA_VERSION]},
    }))
}

/// Whether a job's saved reply schema offers this outcome (fac-616).
pub fn offers(schema: &Value, outcome: Outcome) -> bool {
    schema
        .pointer("/properties/outcome/enum")
        .and_then(Value::as_array)
        .is_some_and(|all| all.iter().any(|o| o.as_str() == Some(outcome.as_str())))
}

/// The version a job's reply schema carries (2.3.6): `schema`'s one allowed value. A schema
/// with none (the tests' echo role) is of this build's version.
pub fn version_of(schema: &Value) -> u32 {
    schema
        .pointer("/properties/schema/enum/0")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(SCHEMA_VERSION)
}

/// The tests' echo role (1.10.4): one message.
pub fn echo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"message": {"type": "string"}},
        "required": ["message"],
        "additionalProperties": false
    })
}

/// A closing turn's feedback about the seat's project (fac-287): what it didn't file, or "nothing".
pub const FEEDBACK_ANSWER: &str = "feedback";

/// A closing turn's feedback about Mork (fac-593): what it didn't file, or "nothing".
pub const MORK_FEEDBACK_ANSWER: &str = "mork_feedback";

/// The reply schema of a closing turn (fac-287, fac-592, fac-725).
pub fn closing_schema(with_mork: bool) -> Value {
    if with_mork {
        object(json!({FEEDBACK_ANSWER: string(), MORK_FEEDBACK_ANSWER: string()}))
    } else {
        object(json!({FEEDBACK_ANSWER: string()}))
    }
}
