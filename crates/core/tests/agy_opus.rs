// fac-661: Opus 5.5 on agy, unnamed-CLI tie-break, per-model efforts, pool notices and group-scoped quota waits.

// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::Sim;
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::config::{AGY_3P_BUCKET, Cli, Effort, ModelOverride, OPUS, PickModel, Picks, SeatOverride};
use mork_types::state::{AgentStatus, Sender};
use mork_types::usage::Limit;
use mork_types::{AgentKey, PlaceId};
use serde_json::json;

const CARD: &str = "web-1";
const FOREMAN: &str = "web-1/foreman";

fn limit(bucket: &str, window: &str, resets_at: i64, reached: bool, seen_ms: i64) -> Limit {
    Limit {
        cli: Cli::Agy,
        account: None,
        bucket: bucket.into(),
        window: window.into(),
        percent: Some(if reached { 100 } else { 10 }),
        resets_at: Some(resets_at),
        reached,
        seen_ms,
    }
}

#[test]
fn agy_opus_picks_at_high_refuses_xhigh_and_unnamed_cli_stays_claude() {
    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD, "model": "opus", "effort": "high"}})).unwrap();
    let planner = &s.state.agents["web-1/planner"];
    let pending = planner.pending.as_ref().expect("start selection pending");
    assert_eq!(pending.cli, Cli::Claude);
    assert_eq!(pending.model, OPUS);

    s.operator(json!({"pick": {"agent": FOREMAN, "model": "opus", "effort": "high"}})).unwrap();
    let picked = s.state.agents[FOREMAN].pending.as_ref().expect("foreman pick pending");
    assert_eq!((picked.cli, picked.model.as_str(), picked.effort), (Cli::Claude, OPUS, Effort::High));

    s.operator(json!({"pick": {"agent": FOREMAN, "cli": "agy", "model": OPUS, "effort": "high"}})).unwrap();
    let agy = s.state.agents[FOREMAN].pending.as_ref().expect("agy pick pending");
    assert_eq!((agy.cli, agy.model.as_str(), agy.effort), (Cli::Agy, OPUS, Effort::High));

    let err =
        s.operator(json!({"pick": {"agent": FOREMAN, "cli": "agy", "model": OPUS, "effort": "xhigh"}})).unwrap_err();
    assert!(err.message.contains("takes no `xhigh`") && err.message.contains("low, medium or high"), "{}", err.message);
}

#[test]
fn agy_opus_config_default_effort_and_child_pick_options_exclude_xhigh() {
    let mut s = Sim::new();
    s.config.seats.insert(
        "planner".into(),
        SeatOverride { cli: Some(Cli::Agy), model: Some(OPUS.into()), ..SeatOverride::default() },
    );
    s.config.seats.insert(
        "foreman".into(),
        SeatOverride {
            cli: Some(Cli::Agy),
            model: Some(OPUS.into()),
            effort: Some(Effort::Xhigh),
            ..SeatOverride::default()
        },
    );
    let mut picks = Picks::default();
    picks.researcher.push(PickModel {
        cli: Cli::Agy,
        model: OPUS.into(),
        label: "opus 5.5".into(),
        efforts: vec![Effort::High, Effort::Xhigh],
    });
    s.config.picks = Some(picks);
    assert_eq!(s.config.seat("planner").unwrap().effort, Effort::High);
    assert_eq!(s.config.seat("foreman").unwrap().effort, Effort::Xhigh);

    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    let planner = &s.state.agents["web-1/planner"];
    let foreman = &s.state.agents[FOREMAN];
    assert_eq!(mork_core::agents::models::launching(&s.state, &s.config, planner).unwrap().effort, Effort::High);
    assert!(mork_core::agents::models::launching(&s.state, &s.config, foreman).is_none());

    s.operator(json!({"say": {"place": CARD, "text": "Check."}})).unwrap();
    let plan = mork_core::agents::turns::plan(&s.state, &AgentKey::new(FOREMAN), s.ctx(), false).unwrap();
    assert!(plan.prompt.contains("For a researcher: opus 5.5 (claude-opus-5-5, agy) at high."), "{}", plan.prompt);

    let from = Sender::Agent(AgentKey::new(FOREMAN));
    let bad = s.cmd(
        from.clone(),
        json!({"spawn": {"card": CARD, "role": "researcher", "task": "Check", "cli": "agy", "model": OPUS, "effort": "xhigh"}}),
    );
    assert!(bad.unwrap_err().message.contains("low, medium or high"));
    s.cmd(
        from,
        json!({"spawn": {"card": CARD, "role": "researcher", "task": "Check", "cli": "agy", "model": OPUS, "effort": "high"}}),
    )
    .unwrap();
}

#[test]
fn agy_reached_notices_name_both_pools_and_resetless_wait_filters_by_group() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.operator(json!({"pick": {"agent": FOREMAN, "cli": "agy", "model": OPUS, "effort": "high"}})).unwrap();
    let gemini_reset = s.now / 1000 + 3600;
    let third_party_reset = s.now / 1000 + 7200;

    s.apply(Event::LimitsSeen {
        limits: vec![limit("account", "5h", gemini_reset, true, s.now)],
        at: None,
        whole: Some(Cli::Agy),
    });
    let main = s.messages(PlaceId::new("main").as_str());
    assert!(main.last().unwrap().text.contains("agy gemini 5h"), "{}", main.last().unwrap().text);

    // An Opus seat rejected without a reset ignores the reached Gemini window and falls back to +60s.
    let id = s.turn_started(FOREMAN);
    let session = SessionSeen { started: true, limit_reached: true, ..SessionSeen::default() };
    s.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Err("quota".into()), session: session.clone() } });
    assert_eq!(s.state.agents[FOREMAN].status, AgentStatus::Waiting { until: s.now + 60_000 });

    // Once the 3p group is reached, main names `agy 3p` and the Opus seat waits until its reset.
    s.apply(Event::LimitsSeen {
        limits: vec![
            limit("account", "5h", gemini_reset, true, s.now),
            limit(AGY_3P_BUCKET, "5h", third_party_reset, true, s.now),
        ],
        at: None,
        whole: Some(Cli::Agy),
    });
    let main = s.messages(PlaceId::new("main").as_str());
    assert!(main.last().unwrap().text.contains("agy 3p 5h"), "{}", main.last().unwrap().text);

    s.now += 61_000;
    let id = s.turn_started(FOREMAN);
    s.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Err("quota".into()), session } });
    assert_eq!(s.state.agents[FOREMAN].status, AgentStatus::Waiting { until: third_party_reset * 1000 });
}

#[test]
fn effort_only_pick_releases_held_report_agy_opus_without_falling_back_to_work() {
    let mut s = Sim::new();
    s.config.seats.insert(
        "foreman".into(),
        SeatOverride {
            report: Some(ModelOverride { cli: Some(Cli::Agy), model: Some(OPUS.into()), effort: Some(Effort::Xhigh) }),
            ..SeatOverride::default()
        },
    );
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD, "mode": "report"}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, mork_types::work::Purpose::Clone { .. }))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();
    let foreman = &s.state.agents[FOREMAN];
    assert!(mork_core::agents::models::launching(&s.state, &s.config, foreman).is_none());

    let shown = mork_core::agents::models::setting(&s.state, &s.config, foreman).unwrap();
    assert_eq!((shown.model.cli, shown.model.model.as_str(), shown.effort), (Cli::Agy, OPUS, Effort::Xhigh));

    s.operator(json!({"pick": {"agent": FOREMAN, "effort": "high"}})).unwrap();
    let foreman = &s.state.agents[FOREMAN];
    let pending = foreman.pending.as_ref().expect("effort-only pick pending");
    assert_eq!((pending.cli, pending.model.as_str(), pending.effort), (Cli::Agy, OPUS, Effort::High));
    let next = mork_core::agents::models::launching(&s.state, &s.config, foreman).expect("held seat released");
    assert_eq!((next.cli, next.model.as_str(), next.effort), (Cli::Agy, OPUS, Effort::High));
}

#[test]
fn pending_surfaces_held_refreshed_default_matching_selection() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    let foreman = s.state.agents.get_mut(FOREMAN).unwrap();
    foreman.choice.as_mut().unwrap().phase = None;
    s.config.seats.insert(
        "foreman".into(),
        SeatOverride {
            cli: Some(Cli::Agy),
            model: Some(OPUS.into()),
            effort: Some(Effort::Xhigh),
            ..SeatOverride::default()
        },
    );
    let foreman = &s.state.agents[FOREMAN];
    assert!(mork_core::agents::models::launching(&s.state, &s.config, foreman).is_none());
    let pending = mork_core::agents::models::pending(&s.state, &s.config, foreman).expect("held default pending");
    assert_eq!((pending.model.cli, pending.model.model.as_str(), pending.effort), (Cli::Agy, OPUS, Effort::Xhigh));

    let err = s.operator(json!({"pick": {"agent": FOREMAN, "effort": "xhigh"}})).unwrap_err();
    assert!(err.message.contains("low, medium or high"), "{}", err.message);
    s.operator(json!({"pick": {"agent": FOREMAN, "effort": "high"}})).unwrap();
    let next = mork_core::agents::models::launching(&s.state, &s.config, &s.state.agents[FOREMAN]).unwrap();
    assert_eq!((next.cli, next.model.as_str(), next.effort), (Cli::Agy, OPUS, Effort::High));
}
