use super::*;
use serde_json::{Value, json};

fn history(mode: &str, rows: &[(i64, &str, Value)]) -> History {
    History {
        project: "example".into(),
        mode: mode.into(),
        legacy: None,
        rows: rows
            .iter()
            .map(|(seq, kind, data)| Event { seq: *seq, time: *seq, kind: (*kind).into(), data: data.clone() })
            .collect(),
    }
}

fn build_histories() -> Histories {
    let mut cards = Histories::new();
    let build =
        json!({"agent":"a/foreman","job":"build","cli":"codex","model":"builder","effort":"high","delta":2_000_000});
    let mut closing = build.clone();
    closing["job"] = json!("close");
    let tool = json!({"agent":"r/foreman","cli":"claude","model":"review","effort":"high","delta":500_000,"unknown":true,
        "tools":{"calls":4,"failed":1,"by":{"Bash":1}}});
    cards.insert(
        "a".into(),
        history(
            "full",
            &[
                (1, "card.start", json!({"run":1,"mode":"full"})),
                (2, "cost", build),
                (3, "reply", json!({"agent":"a/foreman","outcome":"built"})),
                (4, "build.requested", json!({"job":"build"})),
                (5, "build.submitted", json!({"head":"abc"})),
                (8, "cost", json!({"agent":"a/foreman","model":"chat","delta":0})),
                (12, "merge.tested", json!({})),
                (13, "merge.tested", json!({})),
                (15, "card.landed", json!({})),
                (16, "cost", closing),
                (18, "card.start", json!({"run":2,"mode":"full"})),
            ],
        ),
    );
    cards.insert(
        "r".into(),
        history(
            "review",
            &[
                (6, "card.start", json!({"run":1,"mode":"review","of":"a"})),
                (7, "review.asked", json!({"n":1,"submitted":"abc"})),
                (9, "cost", tool),
                (10, "review.verdict", json!({"n":1,"passed":false,"named":[{"n":1,"round":1,"by":"foreman"}]})),
                (11, "review.resolved", json!({"n":1,"declined":true})),
                (
                    19,
                    "cost",
                    json!({"agent":"r/foreman","cli":"claude","model":"review","effort":"high","delta":250_000}),
                ),
            ],
        ),
    );
    cards.insert(
        "b".into(),
        history("full", &[(20, "card.start", json!({"mode":"full"})), (27, "merge.tested", json!({}))]),
    );
    cards.insert(
        "s".into(),
        history(
            "review",
            &[
                (21, "card.start", json!({"mode":"review","of":"b"})),
                (22, "review.asked", json!({"n":1})),
                (23, "review.verdict", json!({"n":1,"passed":false})),
                (26, "review.verdict", json!({"n":2,"passed":true})),
            ],
        ),
    );
    cards
}

#[test]
fn verification_and_landing_have_separate_cohorts_and_costs_follow_the_work() {
    let q = aggregate(9, &build_histories());
    let builder = q.builders.iter().find(|r| r.group.model == "codex builder").unwrap();
    assert_eq!(
        (builder.counts.verified, builder.counts.first_pass, builder.counts.landed, builder.counts.median_rounds),
        (1, 1, 1, Some(1.0))
    );
    let waiting = q.builders.iter().find(|r| r.group.model == "unknown").unwrap();
    assert_eq!((waiting.counts.verified, waiting.counts.first_pass, waiting.counts.landed), (1, 0, 0));
    let review = q.spend.iter().find(|r| r.group.role == "review foreman").unwrap();
    assert_eq!((review.spend.average_dollars, review.spend.cards, review.spend.unknown), (0.75, 1, true));
    assert_eq!(q.spend.iter().find(|r| r.group.model == "codex builder").unwrap().spend.average_dollars, 4.0);
    assert_eq!((q.tools[0].counts.turns, q.tools[0].counts.percent), (1, Some(25.0)));
    assert_eq!(q.findings[0].counts.declined_percent, Some(100.0));
}

#[test]
fn carried_findings_keep_their_author_but_reused_numbers_start_again() {
    let named = json!([{"n":1,"round":1,"by":"reviewer-1"}]);
    let mut cards = Histories::new();
    cards.insert(
        "r".into(),
        history(
            "review",
            &[
                (1, "card.start", json!({"pr":7,"mode":"review","run":1})),
                (2, "review.asked", json!({"n":1})),
                (3, "child.spawned", json!({"agent":"r/reviewer-1","topic":"Correctness"})),
                (4, "cost", json!({"agent":"r/reviewer-1","cli":"codex","model":"reviewer","effort":"high"})),
                (5, "reply", json!({"agent":"r/reviewer-1","outcome":"review-fail"})),
                (6, "cost", json!({"agent":"r/reviewer-1","cli":"agy","model":"other"})),
                (7, "review.verdict", json!({"n":1,"named":named})),
                (9, "card.start", json!({"mode":"review","pr":7,"run":2})),
                (12, "review.verdict", json!({"n":2,"named":named})),
                (15, "review.resolved", json!({"n":1,"declined":false})),
                (17, "card.start", json!({"mode":"review","of":"a","run":3})),
                (19, "review.verdict", json!({"n":1,"named":named})),
                (20, "review.resolved", json!({"n":1,"declined":true})),
            ],
        ),
    );
    cards.insert(
        "a".into(),
        history(
            "full",
            &[
                (11, "review.stalemate", json!({"review":"r","findings":[1]})),
                (14, "review.stalemate", json!({"review":"r","findings":[1]})),
            ],
        ),
    );
    let q = aggregate(0, &cards);
    assert_eq!(q.findings.len(), 2);
    let row = q.findings.iter().find(|r| r.lens == "correctness").unwrap();
    assert_eq!(row.group.model, "codex reviewer");
    assert_eq!((row.counts.findings, row.counts.fixed, row.counts.stalled, row.counts.covered), (1, 1, 1, 1));
    let reset = q.findings.iter().find(|r| r.lens == "unknown").unwrap();
    assert_eq!((reset.counts.findings, reset.counts.declined, reset.counts.stalled), (1, 1, 0));
}

#[test]
fn legacy_findings_require_a_matching_origin_in_the_retained_review() {
    let mut review = history(
        "review",
        &[
            (1, "card.start", json!({"mode":"review","of":"a"})),
            (2, "review.verdict", json!({"n":1,"commit":"h1"})),
            (3, "card.start", json!({"mode":"review","of":"b","run":2})),
            (4, "review.verdict", json!({"n":1,"commit":"h2"})),
            (5, "review.verdict", json!({"n":2,"commit":"other"})),
        ],
    );
    let round = |n, head| json!({"n":n,"commit":head,"base":"b","files":{},"carried":{}});
    let finding = |n| json!({"n":n,"round":n,"by":"foreman","file":"f","line":1,"summary":"s","resolved":true});
    review.legacy = Some(
        serde_json::from_value(json!({"rounds":[round(1,"h2"),round(2,"different")],"raised":[finding(1),finding(2)]}))
            .unwrap(),
    );
    let q = aggregate(0, &BTreeMap::from([("r".into(), review)]));
    assert_eq!(q.findings.len(), 1);
    let row = &q.findings[0];
    assert_eq!(row.lens, "unknown");
    assert_eq!((row.counts.findings, row.counts.fixed_percent, row.counts.stalemate_percent), (1, Some(100.0), None));
}
