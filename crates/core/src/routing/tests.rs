use super::{choose, observe, posterior::Population, sample::Draw};
use crate::views::quality::{Event, History};
use mork_types::config::{Cli, Effort, OPUS, SOL};
use mork_types::routing::{Arm, Estimate, Levels, Metric, Statistics};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn arm(model: &str) -> Arm {
    Arm { cli: if model == SOL { Cli::Codex } else { Cli::Claude }, model: model.to_owned(), effort: Effort::High }
}

#[test]
fn beta_draws_repeat_and_preserve_the_mean() {
    let mut a = Draw::new("seat-1");
    let mut b = Draw::new("seat-1");
    let mut total = 0.0;
    for _ in 0..10000 {
        let draw = a.beta(0.09, 7.0);
        assert_eq!(draw, b.beta(0.09, 7.0));
        total += draw;
    }
    assert!((total / 10000.0 - 0.09).abs() < 0.006);
    assert_eq!(observe::decay(30 * 86_400_000, 0), 0.5);
    assert_eq!(choose::price(100), 0.0);
}

#[test]
fn global_and_leaf_estimates_shrink_observations_and_stay_finite() {
    let a = arm(SOL);
    let mut stats = Statistics::default();
    let counts = Levels {
        global: Estimate { sum: 10.0, weight: 10.0 },
        leaves: BTreeMap::from([("web".into(), Estimate { sum: 0.0, weight: 2.0 })]),
    };
    stats.outcomes.entry(("researcher".into(), Metric::Reliability)).or_default().insert(a.clone(), counts);
    let pop = Population { stats: &stats, role: "researcher", mean_index: 54.84 };
    let [g, l] = pop.levels(Metric::Reliability, &a, Some("web"));
    assert_eq!(g.weight, 17.0);
    assert_eq!(l.weight, 9.0);
    assert!((l.mean - 0.95 * 7.0 / 9.0).abs() < 1e-12);
    assert!(g.mean < 1.0);
    let prior = pop.levels(Metric::Quality, &a, Some("new"))[1];
    assert_eq!(prior.mean, 0.5);
    let zero = Levels { global: Estimate { sum: 0.0, weight: 2.0 }, leaves: Default::default() };
    stats.outcomes.entry(("researcher".into(), Metric::Cost)).or_default().insert(a.clone(), zero);
    let pop = Population { stats: &stats, role: "researcher", mean_index: 54.84 };
    assert!(pop.levels(Metric::Cost, &a, None)[0].mean > 0.0);
}

fn history(mode: &str, rows: Vec<(i64, &str, Value)>) -> History {
    History {
        project: "web".into(),
        mode: mode.into(),
        legacy: None,
        rows: rows.into_iter().map(|(seq, kind, data)| Event { seq, time: 0, kind: kind.into(), data }).collect(),
    }
}

fn cost(id: &str, job: &str, model: &str, delta: u64) -> Value {
    let arm = arm(model);
    json!({"agent":id,"job":job,"cli":arm.cli,"model":model,"effort":"high","ran":true,"by":"mork.toml","delta":delta})
}

#[test]
fn override_shadow_and_tools_follow_jobs_that_reached_the_model() {
    let agent = "a/researcher-1";
    let mut failed = cost(agent, "x", OPUS, 0);
    failed["ran"] = json!(false);
    let mut first = cost(agent, "a", SOL, 100);
    first["by"] = json!("operator");
    first["tools"] = json!({"calls":4,"failed":1});
    let mut next = cost(agent, "b", OPUS, 200);
    next["by"] = json!("operator");
    next["tools"] = json!({"calls":1,"failed":0});
    let rows = vec![
        (1, "card.start", json!({"mode":"report"})),
        (2, "route", json!({"agent":agent,"role":"researcher","mode":"shadow","pick":arm(SOL)})),
        (3, "cost", failed),
        (4, "cost", first),
        (5, "pick", json!({"agent":agent})),
        (6, "pick", json!({"agent":agent})),
        (7, "cost", next.clone()),
        (8, "cost", next),
        (9, "card.stage", json!({"stage":"done"})),
    ];
    let stats = observe::aggregate(0, &BTreeMap::from([("a".into(), history("report", rows))]));
    assert_eq!(stats.shadow["researcher"], [1, 1]);
    let reliability = &stats.outcomes[&("researcher".into(), Metric::Reliability)];
    assert_eq!((reliability[&arm(SOL)].global.sum, reliability[&arm(SOL)].global.weight), (0.75, 3.0));
    assert_eq!(reliability[&arm(OPUS)].global.weight, 1.0);
    assert_eq!(stats.outcomes[&("researcher".into(), Metric::Cost)][&arm(SOL)].global.sum, 0.0005);
}

#[test]
fn verification_charges_rework_to_round_one_and_scores_the_approved_planner() {
    let rows = vec![
        (1, "card.start", json!({"mode":"full"})),
        (2, "cost", cost("a/planner", "p", OPUS, 50)),
        (3, "reply", json!({"agent":"a/planner","outcome":"brief-ready"})),
        (4, "challenge.answered", json!({"objections":true})),
        (5, "brief.approved", json!({})),
        (6, "cost", cost("a/foreman", "initial", SOL, 500000)),
        (7, "build.requested", json!({"job":"initial"})),
        (8, "build.submitted", json!({"head":"abc"})),
        (12, "card.rebriefed", json!({})),
        (13, "cost", cost("a/foreman", "fix", OPUS, 2000000)),
        (16, "merge.tested", json!({})),
        (17, "card.landed", json!({})),
    ];
    let reviews = vec![
        (9, "card.start", json!({"of":"a","mode":"review"})),
        (10, "review.asked", json!({"n":1,"submitted":"abc"})),
        (11, "review.verdict", json!({"n":1,"named":[]})),
        (15, "review.verdict", json!({"n":2,"named":[]})),
    ];
    let stats = observe::aggregate(
        0,
        &BTreeMap::from([("a".into(), history("full", rows)), ("r".into(), history("review", reviews))]),
    );
    assert_eq!(stats.outcomes[&("foreman.work".into(), Metric::Cost)][&arm(SOL)].global.sum, 2.5);
    assert_eq!(stats.outcomes[&("foreman.work".into(), Metric::Quality)][&arm(SOL)].global.sum, 0.0);
    assert_eq!(stats.outcomes[&("planner".into(), Metric::Quality)][&arm(OPUS)].global.sum, 0.125);
    assert_eq!(stats.partners[&("foreman.work".into(), SOL.into(), OPUS.into())].weight, 1.0);
}

#[test]
fn catch_credits_the_bound_jobs_and_adjusts_for_topic_opportunity() {
    for k in [2, 3] {
        let mut rows = vec![(0, "card.start", json!({"mode":"review"})), (1, "review.asked", json!({"n":1}))];
        for i in 0..k {
            let by = format!("r/reviewer-{i}");
            let job = format!("j{i}");
            let topic = ["correctness and tests", "performance", "money, permissions or data integrity"][i];
            let base = 2 + 4 * i as i64;
            rows.push((base, "child.spawned", json!({"topic":topic,"agent":by})));
            rows.push((base + 1, "cost", cost(&by, &job, if i == 0 { SOL } else { OPUS }, 100)));
            rows.push((
                base + 2,
                "reply",
                json!({"agent":by,"job":job,"round":1,"outcome":if i==0 {"review-fail"} else {"review-pass"}}),
            ));
        }
        rows.extend([
            (15, "cost", cost("r/reviewer-1", "late", SOL, 100)),
            (16, "reply", json!({"agent":"r/reviewer-1","job":"late","round":null,"outcome":"review-pass"})),
            (
                17,
                "review.verdict",
                json!({"n":1,"named":[{"n":1,"by":"reviewer-0","round":1},{"n":2,"by":"reviewer-0","round":1}]}),
            ),
            (18, "review.resolved", json!({"n":1})),
            (19, "review.resolved", json!({"n":2,"declined":true})),
        ]);
        let stats = observe::aggregate(0, &BTreeMap::from([("r".into(), history("review", rows))]));
        let catch = &stats.outcomes[&("reviewer".into(), Metric::Catch)];
        assert_eq!(catch[&arm(SOL)].leaves["correctness"].sum, 1.0);
        let performance = catch[&arm(OPUS)].leaves["performance"];
        assert_eq!(performance.sum, 0.0);
        assert!(performance.weight > 0.0 && performance.weight < 1.0 / (k - 1) as f64);
        let precision = stats.outcomes[&("reviewer".into(), Metric::Precision)][&arm(SOL)].global;
        assert_eq!((precision.sum, precision.weight), (1.0, 2.0));
        assert_eq!(super::lens("Design and simplicity"), "design");
        assert_eq!(super::lens("localization"), "other");
    }
}
