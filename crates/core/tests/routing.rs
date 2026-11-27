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

#[test]
fn price_exploration_and_partner_history_change_the_winning_model() {
    use mork_core::routing::choose::{Inputs, Request};
    use mork_types::config::{PickModel, SeatPhase};
    use mork_types::routing::{Arm, Estimate, Levels, Metric, Statistics};
    let mut sim = configured("foreman", "auto", true);
    let limit = sim.state.limits.values_mut().next().unwrap();
    limit.percent = Some(65);
    limit.resets_at = Some((sim.now + 3 * 86_400_000) / 1000);
    let seat = sim.config.seat("foreman").unwrap();
    let expensive = "gpt-6.2-astra";
    let candidates = [SOL, expensive].map(|model| PickModel {
        cli: Cli::Codex,
        model: model.into(),
        label: model.into(),
        efforts: vec![Effort::High],
    });
    let req = Request {
        agent: "web-1/foreman",
        role: "foreman.work",
        run: 1,
        phase: SeatPhase::Work,
        leaf: Some("web"),
        partner: None,
    };
    let choose = |stats: &Statistics, req: &Request<'_>| {
        let inputs = Inputs { now: sim.now, limits: &sim.state.limits, account: None, stats: Some(stats) };
        inputs.choose(req, &seat, &candidates, false)
    };
    let counts = |mean: f64| {
        let e = Estimate { sum: mean * 100_000.0, weight: 100_000.0 };
        Levels { global: e, leaves: std::collections::BTreeMap::from([("web".to_owned(), e)]) }
    };
    let arm = |model: &str| Arm { cli: Cli::Codex, model: model.into(), effort: Effort::High };
    let mut stats = Statistics::default();
    let quality = ("foreman.work".to_owned(), Metric::Quality);
    stats.outcomes.entry(quality.clone()).or_default().insert(arm(expensive), counts(0.9));
    assert!((1..100).any(|run| choose(&stats, &Request { run, ..req }).best().unwrap().offer.model == SOL));

    for model in [SOL, expensive] {
        stats.outcomes.entry(quality.clone()).or_default().insert(arm(model), counts(0.5));
    }
    let ordinary = choose(&stats, &req);
    assert!(ordinary.candidates.iter().all(|c| c.offer.open && c.offer.spare == 0));
    assert_eq!(ordinary.best().unwrap().offer.model, SOL);
    stats
        .partners
        .insert(("foreman.work".into(), expensive.into(), OPUS.into()), Estimate { sum: 1000.0, weight: 1000.0 });
    let paired = choose(&stats, &Request { partner: Some(OPUS), ..req });
    assert_eq!(paired.best().unwrap().offer.model, expensive);
}

#[test]
fn ordinary_researcher_can_delegate_selection_in_auto_mode() {
    for mode in ["fixed", "shadow", "auto"] {
        let mut sim = configured("researcher", mode, true);
        sim.through_brief("web-1", "web");
        let spawn = |sim: &mut Sim, options: Value| {
            let mut cmd = json!({"spawn":{"card":"web-1","role":"researcher","task":"Trace the caller"}});
            for (k, v) in options.as_object().unwrap() {
                cmd["spawn"][k] = v.clone();
            }
            sim.cmd(mork_types::state::Sender::Agent("web-1/foreman".into()), cmd)
        };
        assert!(spawn(&mut sim, json!({"effort":"high"})).is_err());
        let first = spawn(&mut sim, json!({}));
        assert_eq!(first.is_ok(), mode == "auto");
        if mode == "auto" {
            assert_eq!(routes(&sim, "web-1/researcher-1")[0]["pick"]["model"], SOL);
        }
        let selected = spawn(&mut sim, json!({"model":SOL,"effort":"high"}));
        assert!(selected.is_ok(), "{selected:?}");
        let key = if mode == "auto" { "web-1/researcher-2" } else { "web-1/researcher-1" };
        assert_eq!(routes(&sim, key).len(), usize::from(mode == "shadow"));
    }
}
