// why: a missing row or refused setup command fails the fixture.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Sim, reply};
use mork_core::agents::models;
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::config::{Cli, Effort, OPUS, SOL};
use mork_types::usage::Limit;
use serde_json::{Value, json};

fn configured(role: &str, mode: &str, open: bool) -> Sim {
    let mut sim = Sim::new();
    let entry = json!({"cli":"codex","model":SOL,"efforts":["high"],"label":"sol"});
    sim.config.seats.insert(role.into(), serde_json::from_value(json!({"routing":mode,"wall":"full"})).unwrap());
    sim.config.picks = Some(
        serde_json::from_value(json!({
            "planner":[entry],"reviewer":{"inside":[entry]},"researcher":[entry]
        }))
        .unwrap(),
    );
    if open {
        let limit = Limit {
            cli: Cli::Codex,
            bucket: "account".into(),
            window: "week".into(),
            percent: Some(0),
            reached: false,
            resets_at: None,
            seen_ms: sim.now,
            account: None,
        };
        sim.state.limits.insert(limit.key(), limit);
    }
    sim
}

fn routes<'a>(sim: &'a Sim, key: &str) -> Vec<&'a Value> {
    sim.journal.iter().filter(|(_, k, _, d)| k == "route" && d["agent"] == key).map(|r| &r.3).collect()
}

#[test]
fn start_routes_once_and_explicit_selections_win() {
    let key = "web-1/planner";
    for (mode, open, explicit, expected) in [
        ("fixed", true, false, OPUS),
        ("shadow", true, false, OPUS),
        ("auto", true, false, SOL),
        ("auto", false, false, OPUS),
        ("auto", true, true, OPUS),
        ("shadow", true, true, OPUS),
    ] {
        let mut sim = configured("planner", mode, open);
        sim.board("web-1", "web");
        let mut start = json!({"start":{"card":"web-1"}});
        if explicit {
            start["start"]["model"] = json!(OPUS);
            start["start"]["effort"] = json!("high");
        }
        sim.operator(start).unwrap();
        sim.settle_actions();
        let agent = &sim.state.agents[key];
        let launched = models::launching(&sim.state, &sim.config, agent).unwrap();
        assert_eq!(launched.model, expected, "{mode}, open {open}, explicit {explicit}");
        let rows = routes(&sim, key);
        assert_eq!(rows.len(), usize::from(mode == "shadow" || mode == "auto" && !explicit));
        if let Some(row) = rows.first() {
            if open {
                assert_eq!(row["pick"]["model"], SOL);
            } else {
                assert!(row["pick"].is_null());
            }
        }
        if mode == "auto" && open && !explicit {
            assert_eq!(launched.effort, Effort::High);
            assert_eq!(agent.choice.as_ref().unwrap().by, "router");
        }
    }
}

#[test]
fn spawned_reviewer_verdict_identifies_its_round_job_and_router_choice() {
    let mut sim = configured("reviewer", "auto", true);
    sim.through_brief("web-1", "web");
    sim.built("web-1", "h1", "base1");
    sim.checks("web-1", 0, &[]);
    sim.candidate("web-1");
    sim.round_asked("web-1");
    let rc = sim.review_card("web-1");
    let key = format!("{rc}/reviewer-1");
    let foreman = format!("{rc}/foreman");
    sim.turn_sending(
        &foreman,
        &[json!({"spawn":{"card":rc,"role":"reviewer","topic":"performance","task":"Review runtime"}})],
        reply("working", "Review"),
    );
    let row = routes(&sim, &key)[0];
    assert_eq!(row["leaf"], "performance");
    assert_eq!(row["pick"]["effort"], "high");
    let (id, _) = sim.turn_started_with_argv(&key, &["codex", "-m", SOL, "--effort", "high"]);
    let end = JobEnd::Finished {
        reply: Ok(reply("review-pass", "Passed")),
        session: SessionSeen { started: true, ..Default::default() },
    };
    sim.apply(Event::JobEnded { id: id.clone(), end });
    for (kind, field, value) in
        [("cost", "by", json!("router")), ("cost", "ran", json!(true)), ("reply", "round", json!(1))]
    {
        let row = sim.journal.iter().find(|(_, k, _, r)| k == kind && r["job"] == id.as_str()).unwrap();
        assert_eq!(row.3[field], value);
    }
    let mut poster = mork_core::post::Poster::new(&sim.state, sim.now);
    models::reround(&mut poster, &rc.into());
    let seat = poster.agent(&key.as_str().into()).unwrap();
    let kept = seat.choice.unwrap();
    assert_eq!(
        (kept.by.as_str(), kept.effort, kept.phase),
        ("router", Effort::High, Some(mork_types::config::SeatPhase::Work))
    );
}
