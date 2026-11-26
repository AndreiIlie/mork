//! A card run's timeline (4.19): read from its journal, each moment counted once.

use std::collections::BTreeMap;

use mork_types::card::Stage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// The card entered a stage.
    Stage(Stage),
    /// An ask opened, with none open before.
    Waiting,
    /// The last open ask was answered.
    Answered,
    Paused,
    Resumed,
    /// A full card's specification plan was first recapped (fac-413).
    Planning,
    /// The plan was approved.
    Planned,
    /// A mock opened at build: the operator's reopen, or an older build's after the plan's approve.
    Mocking,
    /// The mock ended or was deferred by the flow conversion.
    Mocked,
    /// Verification passed and the card moved into Land (merge queue or PR publication) (fac-680).
    Verified,
    /// Verification restarted outside the merge queue while still in Merging (fac-680).
    Reverify,
    /// The run ended (done or closed).
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub at: i64,
    pub mark: Mark,
}

/// The parts of a run's lifetime: each moment counts once, to `you` while an ask is open, else
/// to `paused`, else to its strip step. Checks count under review (fac-413).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub you: i64,
    pub paused: i64,
    pub stages: BTreeMap<String, i64>,
    pub lifetime: i64,
}

/// The totals of a run from its marks, up to `now` or its end.
/// The marks are taken in the journal's order; a clock set back counts as no time passing.
pub fn totals(points: &[Point], now: i64) -> Totals {
    let mut t = Totals::default();
    let Some(first) = points.first() else { return t };
    let (start, mut at) = (first.at, first.at);
    let (mut stage, mut planning, mut mocking, mut landing) = (None::<Stage>, false, false, false);
    let (mut waiting, mut paused, mut ended) = (false, false, false);
    for pt in points {
        let when = pt.at.max(at);
        count(&mut t, name(stage, planning, mocking, landing), waiting, paused, when - at);
        at = when;
        planning = still_planning(planning, &pt.mark);
        mocking = still_mocking(mocking, &pt.mark);
        match &pt.mark {
            Mark::Stage(s) => {
                if stage != Some(*s) {
                    landing = false;
                }
                stage = Some(*s);
            }
            Mark::Verified => landing = true,
            Mark::Reverify => landing = false,
            Mark::Waiting => waiting = true,
            Mark::Answered => waiting = false,
            Mark::Paused => paused = true,
            Mark::Resumed => paused = false,
            Mark::Planning | Mark::Planned | Mark::Mocking | Mark::Mocked => {}
            Mark::End => {
                ended = true;
                break;
            }
        }
    }
    if !ended {
        let when = now.max(at);
        count(&mut t, name(stage, planning, mocking, landing), waiting, paused, when - at);
        at = when;
    }
    t.lifetime = at - start;
    t
}

fn still_planning(planning: bool, mark: &Mark) -> bool {
    match mark {
        Mark::Planning => true,
        Mark::Planned => false,
        Mark::Stage(s) => planning && *s == Stage::Brief,
        Mark::Waiting
        | Mark::Answered
        | Mark::Paused
        | Mark::Resumed
        | Mark::Mocking
        | Mark::Mocked
        | Mark::Verified
        | Mark::Reverify
        | Mark::End => planning,
    }
}

fn still_mocking(mocking: bool, mark: &Mark) -> bool {
    match mark {
        Mark::Mocking => true,
        Mark::Mocked => false,
        Mark::Stage(s) => mocking && *s == Stage::Build,
        Mark::Waiting
        | Mark::Answered
        | Mark::Paused
        | Mark::Resumed
        | Mark::Planning
        | Mark::Planned
        | Mark::Verified
        | Mark::Reverify
        | Mark::End => mocking,
    }
}

fn name(stage: Option<Stage>, planning: bool, mocking: bool, landing: bool) -> &'static str {
    if mocking {
        "mock"
    } else if planning {
        "plan"
    } else if stage == Some(Stage::Checks) {
        "review"
    } else if stage == Some(Stage::Merging) {
        if landing { "land" } else { "verify" }
    } else {
        stage.map_or("starting", |s| s.as_str())
    }
}

fn count(t: &mut Totals, name: &str, waiting: bool, paused: bool, span: i64) {
    if span <= 0 {
        return;
    }
    if waiting {
        t.you += span;
    } else if paused {
        t.paused += span;
    } else {
        *t.stages.entry(name.to_owned()).or_insert(0) += span;
    }
}

/// A run's marks from its journal rows in insertion order: stages, the specification plan, mock, asks,
/// pauses, its end, and cost in millionths of a dollar.
pub fn points(rows: &[(i64, &str, &serde_json::Value)]) -> (Vec<Point>, u64) {
    let mut out = Vec::new();
    let mut open = std::collections::BTreeSet::new();
    let (mut paused, mut cost) = (false, 0u64);
    for (at, kind, data) in rows {
        let at = *at;
        let mark = match *kind {
            "card.start" => Some(Mark::Stage(Stage::Starting)),
            "card.stage" => {
                if let Some(s) = data.get("stage").and_then(|v| serde_json::from_value::<Stage>(v.clone()).ok()) {
                    out.push(Point { at, mark: Mark::Stage(s) });
                }
                let now = data.get("paused").and_then(serde_json::Value::as_bool) == Some(true);
                let mark = (now != paused).then_some(if now { Mark::Paused } else { Mark::Resumed });
                paused = now;
                mark
            }
            "ask" => {
                let was = !open.is_empty();
                let id = data.get("id").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
                if data.get("open").and_then(serde_json::Value::as_bool) == Some(true) {
                    if data.get("kind").and_then(serde_json::Value::as_str) == Some("approve-brief") {
                        out.push(Point { at, mark: Mark::Planning });
                    }
                    open.insert(id);
                } else {
                    open.remove(&id);
                }
                match (was, open.is_empty()) {
                    (false, false) => Some(Mark::Waiting),
                    (true, true) => Some(Mark::Answered),
                    _ => None,
                }
            }
            "brief.planned" => Some(Mark::Planning),
            "brief.approved" | "brief.clear" => Some(Mark::Planned),
            // A baseline step's mock is the brief's: the strip's mock lights only at build (fac-636).
            "mock.opened" if data.get("baseline").and_then(serde_json::Value::as_bool) == Some(true) => None,
            "mock.opened" | "mock.reopened" => Some(Mark::Mocking),
            "mock.ended" | "mock.deferred" => Some(Mark::Mocked),
            "merge.tested" => Some(Mark::Verified),
            "merge.reverify" => Some(Mark::Reverify),
            "card.landed" | "card.closed" => Some(Mark::End),
            "cost" => {
                cost += data.get("delta").and_then(serde_json::Value::as_u64).unwrap_or(0);
                None
            }
            _ => None,
        };
        if let Some(mark) = mark {
            out.push(Point { at, mark });
        }
    }
    (out, cost)
}

/// Current-run cost and token sums by seat; older lines contribute dollars alone (fac-526).
pub fn seat_costs(rows: &[(i64, &str, &serde_json::Value)]) -> Vec<mork_types::about::SeatCost> {
    let mut sums = BTreeMap::<String, (u64, bool, mork_types::cost::Tokens)>::new();
    for (_, kind, data) in rows {
        if *kind != "cost" {
            continue;
        }
        let Some(agent) = data.get("agent").and_then(serde_json::Value::as_str) else { continue };
        let (micros, unknown, tokens) = sums.entry(agent.to_owned()).or_default();
        *micros = micros.saturating_add(data.get("delta").and_then(serde_json::Value::as_u64).unwrap_or(0));
        *unknown |= data.get("unknown").and_then(serde_json::Value::as_bool) == Some(true);
        let share = data.get("tokens").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        *tokens = tokens.saturating_add(share);
    }
    sums.into_iter()
        .map(|(agent, (micros, unknown, tokens))| mork_types::about::SeatCost {
            agent: mork_types::AgentKey::new(agent),
            cost_cents: micros.div_ceil(10_000),
            unknown,
            tokens,
        })
        .collect()
}
