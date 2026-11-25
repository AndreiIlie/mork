//! Asks (3.5.5, 4.4.5): something waiting on Andrei.

use serde::{Deserialize, Serialize};

use crate::ids::AgentKey;

/// A colour: of a hand, an ask or a light (3.5, 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    Grey,
    Green,
    Yellow,
    Red,
}

/// The kinds of ask (4.4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AskKind {
    ApproveBrief,
    Answer,
    Merge,
    ApprovePr,
    Done,
    Stuck,
    Checks,
    Blocked,
    Error,
    Cut,
    Held,
    Alarm,
    Token,
    NewPr,
    /// A door call waits for Andrei's `once` or `no` (fac-1): his alone.
    Door,
    /// A visual card's mock step waits for Andrei's `approve` of the mock (fac-258): his alone.
    Mock,
    /// A failed review round waits for Andrei's triage when delegation is off or on a delegated stalemate, or an explicitly given orchestrator's (fac-680, fac-689, fac-694).
    Triage,
}

/// Something waiting on Andrei.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub id: crate::AskId,
    pub place: crate::PlaceId,
    pub card: Option<crate::ids::CardId>,
    pub kind: AskKind,
    pub color: Color,
    pub text: String,
    /// The agent whose ask it is, or whose turn raised it.
    pub agent: Option<AgentKey>,
    pub raised_ms: i64,
    pub open: bool,
    /// What it lists below its line: an action's words.
    pub detail: String,
    /// The action it holds for (`held`).
    pub action: Option<crate::ActionId>,
    /// Its desktop notice was claimed in the transaction that raised it (2.6.1).
    pub notice: bool,
    pub answered_ms: Option<i64>,
    /// Given to the project's orchestrator, explicitly or on a delegated card (fac-119, fac-669): marked until closed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub given: bool,
    /// Marked as a design question (fac-233): shown tagged; auto-given like any `answer` ask when delegated.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub design: bool,
    /// Its hand's options and pick (fac-696); `text` holds them composed for readers of words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Choices>,
}

/// An ask's numbered options and the seat's pick (fac-696), as its hand gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choices {
    /// The ask's own words, without its options.
    pub ask: String,
    pub options: Vec<String>,
    /// The option the seat picks, by its number from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

impl Choices {
    /// The ask, its numbered options, then the pick and its reason, as one text.
    pub fn words(&self) -> String {
        let options: Vec<String> = self.options.iter().zip(1..).map(|(o, n)| format!("{n}. {o}")).collect();
        let mut out = format!("{}\n\n{}", self.ask, options.join("\n"));
        match (self.pick, self.reason.as_str()) {
            (Some(n), "") => out.push_str(&format!("\n\nI'd pick {n}.")),
            (Some(n), why) => out.push_str(&format!("\n\nI'd pick {n}: {why}")),
            (None, _) => {}
        }
        out
    }
}

impl Ask {
    /// An open ask, with nothing below its line yet.
    pub fn new(
        id: crate::AskId,
        place: &str,
        card: Option<crate::ids::CardId>,
        kind: AskKind,
        color: Color,
        text: &str,
    ) -> Self {
        Self {
            id,
            place: crate::PlaceId::new(place),
            card,
            kind,
            color,
            text: text.to_owned(),
            agent: None,
            raised_ms: 0,
            open: true,
            detail: String::new(),
            action: None,
            notice: false,
            answered_ms: None,
            given: false,
            design: false,
            choices: None,
        }
    }

    /// Closes it: no longer waiting, nor given to the orchestrator.
    pub fn close(&mut self) {
        self.open = false;
        self.given = false;
    }
}
