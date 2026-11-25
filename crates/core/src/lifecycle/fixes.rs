//! Fix requests: every round goes to the card's foreman at once and is counted; from the third on, the card's room and
//! main are told which round it is. No round is held.

use factory_types::card::{Card, Stage};
use factory_types::reply::{Finding, Role};
use factory_types::state::{MessageKind, Sender};
use factory_types::work::CheckRun;
use serde_json::Value;

use crate::messages::requests::ask_agent;
use crate::post::Poster;

// The first round the room and main are told of.
const NOTICE_FROM: u32 = 3;

/// A fix request: one round, sent to the card's foreman; from the third, said in the room and in main,
/// to Andrei, waking nobody.
pub fn send_back(p: &mut Poster<'_>, mut card: Card, text: &str) {
    card.stage = Stage::Build;
    card.revisions += 1;
    let (id, n) = (card.id.clone(), card.revisions);
    p.put_card(card);
    ask_agent(p, &id, Role::Foreman, text.lines().next().unwrap_or("Fix"), text);
    if n >= NOTICE_FROM {
        let words = format!("{id} is in its {} round", ordinal(n));
        for place in [id.as_str(), crate::messages::places::MAIN] {
            p.post(place, Sender::Engine, Some("andrei"), MessageKind::Engine, "", &words);
        }
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The findings, as the fix request lists them (4.8.3).
pub fn listed(findings: &[Finding]) -> String {
    findings
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{}. {}:{}: {}\nProof: {}", i + 1, f.file, f.line, f.summary, f.proof))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The checks' findings, as their fix request lists them.
pub(super) fn findings_text(run: &CheckRun) -> String {
    let none = Vec::new();
    let fs = run.report.as_ref().and_then(|r| r.get("findings")).and_then(Value::as_array).unwrap_or(&none);
    fs.iter()
        .enumerate()
        .map(|(i, f)| {
            let s =
                |k: &str| f.get(k).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).unwrap_or_default();
            format!("{}. {}:{}: {}\nProof: {}", i + 1, s("file"), s("line"), s("summary"), s("proof"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
