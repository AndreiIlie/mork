// fac-685: 7-day burn metrics (roles, ended runs, folded reviews, cache hit, cards left) and stored-context fallback.

// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::Sim;
use mork_core::views::burn::{self, Row, WEEK_MS, WeekPool};
use mork_core::views::outside::{AgentSeen, Outside};
use mork_core::views::place;
use mork_types::AgentKey;
use mork_types::config::{Cli, Effort, SeatPhase};
use mork_types::state::Choice;
use mork_types::usage::{CardsLeft, RoleBurn};
use serde_json::{Value, json};

fn cost_row(agent: &str, cli: &str, model: &str, delta: u64, unknown: bool, tokens: Option<Value>) -> Value {
    let mut v = json!({"agent": agent, "cli": cli, "model": model, "delta": delta, "unknown": unknown});
    if let Some(t) = tokens {
        v["tokens"] = t;
    }
    v
}

fn pool(name: &str, cli: Cli, bucket: &str, percent: Option<u32>, reset_ms: Option<i64>) -> WeekPool {
    WeekPool {
        name: name.into(),
        cli,
        bucket: bucket.into(),
        percent,
        reached: false,
        stale: false,
        signed_out: false,
        reset_ms,
        length_ms: Some(WEEK_MS),
    }
}

#[test]
fn role_extraction_drops_dm_and_trailing_number() {
    assert_eq!(burn::role_of("fac-1/foreman"), "foreman");
    assert_eq!(burn::role_of("fac-1/foreman/dm"), "foreman");
    assert_eq!(burn::role_of("fac-1/reviewer-2"), "reviewer");
    assert_eq!(burn::role_of("fac-1/researcher-12"), "researcher");
    assert_eq!(burn::role_of("fac-1/baseline-1"), "baseline");
    assert_eq!(burn::role_of("branch-mork-fac-1/foreman"), "foreman");
}

#[test]
fn roles_total_and_cache_hit_across_clis() {
    let now = 20 * 86_400_000i64;
    let since = now - WEEK_MS;
    let c1 = cost_row(
        "fac-1/foreman/dm",
        "claude",
        "claude-opus-5-5",
        10_000_000,
        false,
        Some(json!({"input": 100, "cache_read": 800, "cache_write": 100, "output": 50})),
    );
    let c2 = cost_row(
        "branch-mork-fac-1/reviewer-2",
        "codex",
        "gpt-6.2-sol",
        5_000_000,
        true,
        Some(json!({"input": 50, "cache_read": 950, "cache_write": 0, "output": 40})),
    );
    let c3 = cost_row("fac-2/planner", "agy", "gemini-4-neon", 3_000_000, false, None);
    let codex_miss = json!({"cli": "codex", "model": "gpt-6.2-sol", "tokens": {"input": 700}});
    let agy_miss = json!({"cli": "agy", "model": "claude-opus-5-5", "tokens": {"input": 50}});
    let unattributed = json!({"delta": 10_000, "tokens": {"cache_read": 100}});
    let recent_costs: Vec<Row<'_>> = vec![
        (since + 1000, Some("fac-1"), "cost", &c1),
        (since + 2000, Some("branch-mork-fac-1"), "cost", &c2),
        (since + 3000, Some("fac-2"), "cost", &c3),
        (since + 4000, None, "cost", &codex_miss),
        (since + 5000, None, "cost", &agy_miss),
        (since + 6000, None, "cost", &unattributed),
    ];

    let out = burn::recent(since, &recent_costs, &[], &BTreeMap::new(), &BTreeMap::new(), &[]);
    assert_eq!(out.since_ms, since);
    assert_eq!(out.total_cents, 1801);
    assert!(out.unknown);
    assert_eq!(
        out.roles,
        vec![
            RoleBurn { role: "foreman".into(), cents: 1000, unknown: false },
            RoleBurn { role: "reviewer".into(), cents: 500, unknown: true },
            RoleBurn { role: "planner".into(), cents: 300, unknown: false },
        ]
    );
    assert_eq!(out.ended_cards, 0);
    assert_eq!(out.average_cents, None);
    assert_eq!(
        serde_json::to_value(&out.cache_by_model).unwrap(),
        json!([
            {"cli": "agy", "model": "claude-opus-5-5", "cache_hit_percent": 0},
            {"cli": "agy", "model": "gemini-4-neon"},
            {"cli": "claude", "model": "claude-opus-5-5", "cache_hit_percent": 80},
            {"cli": "codex", "model": "gpt-6.2-sol", "cache_hit_percent": 56}
        ])
    );
    assert_eq!(out.cards_left, None);
}

#[test]
fn average_burn_folds_review_into_parent_and_slices_restarted_runs() {
    let now = 20 * 86_400_000i64;
    let since = now - WEEK_MS;
    let st_fac1 = json!({"stage": "closed", "paused": false, "run": 1});
    let st_rev1 = json!({"stage": "done", "paused": false, "run": 1});
    let st_pr9 = json!({"stage": "done", "paused": false, "run": 1});
    let stages: Vec<Row<'_>> = vec![
        (since + 4000, Some("fac-1"), "card.stage", &st_fac1),
        (since + 4000, Some("branch-mork-fac-1"), "card.stage", &st_rev1),
        (since + 5000, Some("pr-mork-9"), "card.stage", &st_pr9),
    ];

    let fac1_s1 = json!({"run": 1, "mode": "full"});
    let fac1_old = cost_row("fac-1/planner", "claude", "claude-opus-5-5", 2_000_000, false, None);
    let fac1_c1 = cost_row("fac-1/foreman/dm", "claude", "claude-opus-5-5", 10_000_000, false, None);
    let fac1_s2 = json!({"run": 2, "mode": "full"});
    let fac1_live = cost_row("fac-1/foreman", "claude", "claude-opus-5-5", 99_000_000, false, None);
    let rev1_s1 = json!({"run": 1, "mode": "review", "of": "fac-1"});
    let rev1_c = cost_row("branch-mork-fac-1/reviewer-2", "codex", "gpt-6.2-sol", 5_000_000, false, None);
    let pr9_s1 = json!({"run": 1, "mode": "review"});
    let pr9_c = cost_row("pr-mork-9/reviewer-1", "codex", "gpt-6.2-sol", 3_000_000, false, None);

    let mut card_rows: BTreeMap<String, Vec<Row<'_>>> = BTreeMap::new();
    card_rows.insert(
        "fac-1".into(),
        vec![
            (since - 5000, Some("fac-1"), "card.start", &fac1_s1),
            (since - 4000, Some("fac-1"), "cost", &fac1_old),
            (since + 1000, Some("fac-1"), "cost", &fac1_c1),
            (since + 6000, Some("fac-1"), "card.start", &fac1_s2),
            (since + 7000, Some("fac-1"), "cost", &fac1_live),
        ],
    );
    card_rows.insert(
        "branch-mork-fac-1".into(),
        vec![
            (since + 1500, Some("branch-mork-fac-1"), "card.start", &rev1_s1),
            (since + 2000, Some("branch-mork-fac-1"), "cost", &rev1_c),
        ],
    );
    card_rows.insert(
        "pr-mork-9".into(),
        vec![
            (since + 1000, Some("pr-mork-9"), "card.start", &pr9_s1),
            (since + 2000, Some("pr-mork-9"), "cost", &pr9_c),
        ],
    );
    let mut parents = BTreeMap::new();
    parents.insert("branch-mork-fac-1".into(), "fac-1".into());

    let out = burn::recent(since, &[], &stages, &card_rows, &parents, &[]);
    // fac-1 run 1 ($2 + $10 + folded $5 = $17, excluding live run 2) and pr-mork-9 ($3) => $10 average over 2.
    assert_eq!(out.ended_cards, 2);
    assert_eq!(out.average_cents, Some(1000));
}

#[test]
fn reopened_pr_review_without_card_start_slices_each_run_separately() {
    let now = 20 * 86_400_000i64;
    let since = now - WEEK_MS;
    let s1 = json!({"run": 1, "mode": "review"});
    let c1 = cost_row("pr-mork-7/reviewer-1", "codex", "gpt-6.2-sol", 4_000_000, false, None);
    let d1 = json!({"stage": "done", "paused": false, "run": 1});
    let r2 = json!({"stage": "merging", "paused": false, "run": 2});
    let c2 = cost_row("pr-mork-7/reviewer-1", "codex", "gpt-6.2-sol", 6_000_000, false, None);
    let d2 = json!({"stage": "done", "paused": false, "run": 2});

    let mut card_rows: BTreeMap<String, Vec<Row<'_>>> = BTreeMap::new();
    card_rows.insert(
        "pr-mork-7".into(),
        vec![
            (since - 9000, Some("pr-mork-7"), "card.start", &s1),
            (since - 8000, Some("pr-mork-7"), "cost", &c1),
            (since - 7000, Some("pr-mork-7"), "card.stage", &d1),
            (since + 1000, Some("pr-mork-7"), "card.stage", &r2),
            (since + 2000, Some("pr-mork-7"), "cost", &c2),
            (since + 3000, Some("pr-mork-7"), "card.stage", &d2),
        ],
    );
    let stages: Vec<Row<'_>> = vec![(since + 3000, Some("pr-mork-7"), "card.stage", &d2)];
    let out = burn::recent(since, &[], &stages, &card_rows, &BTreeMap::new(), &[]);
    assert_eq!(out.ended_cards, 1);
    assert_eq!(out.average_cents, Some(600));

    // When both run 1 and reopened run 2 ended in the window, each keeps only its own cost ($4 and $6 -> $5 average).
    let both_stages: Vec<Row<'_>> =
        vec![(since + 500, Some("pr-mork-7"), "card.stage", &d1), (since + 3000, Some("pr-mork-7"), "card.stage", &d2)];
    let both = burn::recent(since, &[], &both_stages, &card_rows, &BTreeMap::new(), &[]);
    assert_eq!(both.ended_cards, 2);
    assert_eq!(both.average_cents, Some(500));
}

#[test]
fn cards_left_skips_zero_card_pools_splits_agy_buckets_and_handles_reached_stale_unknown() {
    let now = 20 * 86_400_000i64;
    let since = now - WEEK_MS;
    let reset = now + 3 * 86_400_000;
    let week_start = reset - WEEK_MS;

    let claude_a = cost_row("fac-1/foreman", "claude", "claude-opus-5-5", 1_000_000, false, None);
    let claude_a_rev = cost_row("branch-mork-fac-1/foreman", "claude", "claude-opus-5-5", 1_000_000, false, None);
    let claude_b = cost_row("fac-2/foreman", "claude", "claude-opus-5-5", 1_000_000, false, None);
    let agy_gem = cost_row("fac-3/foreman", "agy", "gemini-4-neon", 1_000_000, false, None);
    let agy_3p = cost_row("fac-4/foreman", "agy", "claude-opus-5-5", 1_000_000, false, None);
    let agy_nomodel = json!({"agent": "fac-5/foreman", "cli": "agy", "delta": 1_000_000, "unknown": false});

    let costs: Vec<Row<'_>> = vec![
        (week_start + 10, Some("fac-1"), "cost", &claude_a),
        (week_start + 20, Some("branch-mork-fac-1"), "cost", &claude_a_rev),
        (week_start + 30, Some("fac-2"), "cost", &claude_b),
        (week_start + 40, Some("fac-3"), "cost", &agy_gem),
        (week_start + 50, Some("fac-4"), "cost", &agy_3p),
        (week_start + 60, Some("fac-5"), "cost", &agy_nomodel),
    ];
    let mut parents = BTreeMap::new();
    parents.insert("branch-mork-fac-1".into(), "fac-1".into());

    // Codex has 0 mork cards so it skips; agy gemini (1 card at 50% -> 1) beats claude max (2 at 40% -> 3) and agy 3p (1 at 20% -> 4).
    // Stale, signed-out, 0%, and unknown-percent pools are skipped.
    let mut stale_pool = pool("stale", Cli::Codex, "account", Some(90), Some(reset));
    stale_pool.stale = true;
    let mut signed_out = pool("claude team", Cli::Claude, "account", Some(95), Some(reset));
    signed_out.signed_out = true;
    let pools = vec![
        pool("codex", Cli::Codex, "account", Some(25), Some(reset)),
        pool("claude max", Cli::Claude, "account", Some(40), Some(reset)),
        pool("agy gemini", Cli::Agy, "account", Some(50), Some(reset)),
        pool("agy 3p", Cli::Agy, "3p", Some(20), Some(reset)),
        pool("zero", Cli::Claude, "account", Some(0), Some(reset)),
        pool("unknown", Cli::Claude, "account", None, Some(reset)),
        stale_pool,
        signed_out,
    ];
    let out = burn::recent(since, &costs, &[], &BTreeMap::new(), &parents, &pools);
    assert_eq!(out.cards_left, Some(CardsLeft { pool: "agy gemini".into(), cards: 1 }));

    // A reached pool with unknown percent reports 0 cards left.
    let mut reached = pool("codex", Cli::Codex, "account", None, Some(reset));
    reached.reached = true;
    let out_reached = burn::recent(since, &costs, &[], &BTreeMap::new(), &parents, &[reached]);
    assert_eq!(out_reached.cards_left, Some(CardsLeft { pool: "codex".into(), cards: 0 }));
}

#[test]
fn seat_row_falls_back_to_stored_context_after_reload_and_prefers_live_stream() {
    let mut s = Sim::new();
    s.through_brief("web-1", "web");
    let key = AgentKey::new("web-1/foreman");
    let a = s.state.agents.get_mut(&key).unwrap();
    a.choice = Some(Choice {
        cli: Cli::Claude,
        model: "claude-opus-5-5".into(),
        effort: Effort::High,
        by: "operator".into(),
        phase: Some(SeatPhase::Work),
    });
    a.context = Some(120_000);
    a.window = Some(("claude-opus-5-5".into(), 1_000_000));

    // Empty `outside` simulates a fresh reload before any turn streams again.
    let view = place::place(&s.state, "web-1", None, &Outside::default(), s.ctx());
    let row = view.agents.iter().find(|r| r.agent == key).unwrap();
    assert_eq!(row.context, Some((120_000, 200_000)));

    // Live stream context overrides the stored fallback.
    let mut outside = Outside::default();
    outside.agents.insert(key.clone(), AgentSeen { context: Some((140_000, 200_000)), ..AgentSeen::default() });
    let live = place::place(&s.state, "web-1", None, &outside, s.ctx());
    let live_row = live.agents.iter().find(|r| r.agent == key).unwrap();
    assert_eq!(live_row.context, Some((140_000, 200_000)));

    // agy ignores a stale Claude window when cap is disabled, and uses cap when enabled.
    let a = s.state.agents.get_mut(&key).unwrap();
    a.choice.as_mut().unwrap().cli = Cli::Agy;
    a.choice.as_mut().unwrap().model = "gemini-4-neon".into();
    s.config.mork.context_cap = 0;
    let agy_uncapped = place::place(&s.state, "web-1", None, &Outside::default(), s.ctx());
    assert_eq!(agy_uncapped.agents.iter().find(|r| r.agent == key).unwrap().context, None);

    s.config.mork.context_cap = 300_000;
    let agy_capped = place::place(&s.state, "web-1", None, &Outside::default(), s.ctx());
    assert_eq!(agy_capped.agents.iter().find(|r| r.agent == key).unwrap().context, Some((120_000, 300_000)));

    // Codex reports no stored context.
    let a = s.state.agents.get_mut(&key).unwrap();
    a.choice.as_mut().unwrap().cli = Cli::Codex;
    a.choice.as_mut().unwrap().model = "gpt-6.2-sol".into();
    let codex = place::place(&s.state, "web-1", None, &Outside::default(), s.ctx());
    assert_eq!(codex.agents.iter().find(|r| r.agent == key).unwrap().context, None);
}

// fac-696: the daily series buckets spend and landings by local day at the screen's offset, oldest day first.
#[test]
fn daily_series_buckets_spend_and_landings_by_local_day() {
    const DAY: i64 = 86_400_000;
    let offset = 120; // UTC+2
    let local_midnight = 20_000 * DAY - 2 * 3_600_000;
    let now = local_midnight + 10 * 3_600_000;
    let spent = cost_row("fac-1/foreman", "claude", "claude-opus-5-5", 1_234_567, false, None);
    let unknown = cost_row("fac-1/foreman", "claude", "claude-opus-5-5", 0, true, None);
    let landed = json!({});
    let costs: Vec<Row<'_>> = vec![
        (local_midnight + 1, Some("fac-1"), "cost", &spent),
        (local_midnight - 1, Some("fac-1"), "cost", &unknown),
        (local_midnight - 14 * DAY, Some("fac-1"), "cost", &spent),
    ];
    let landings: Vec<Row<'_>> = vec![(local_midnight + 5, Some("fac-1"), "card.landed", &landed)];
    let days = burn::daily(now, offset, &costs, &landings);
    assert_eq!(days.len(), 14);
    assert_eq!(days[0].day_ms, local_midnight - 13 * DAY);
    assert_eq!(burn::series_start(now, offset), local_midnight - 13 * DAY);
    let today = &days[13];
    assert_eq!((today.day_ms, today.cents, today.unknown, today.landed), (local_midnight, 124, false, 1));
    assert_eq!((days[12].cents, days[12].unknown, days[12].landed), (0, true, 0));
    assert!(days[..12].iter().all(|d| d.cents == 0 && !d.unknown && d.landed == 0));
}
