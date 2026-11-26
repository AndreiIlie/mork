// Pure state-machine tests for engine-native findings triage, four-step pipeline, and folded branch reviews (fac-680).

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply};
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_core::views::outside::Outside;
use mork_core::views::place::place;
use mork_core::views::screen::screen;
use mork_types::ask::{AskKind, Color};
use mork_types::card::{Landing, Stage};
use mork_types::state::{MessageKind, Sender};
use mork_types::view::{StageAt, SuiteProgress};
use mork_types::work::{Checkout, FinalRun, Purpose};
use serde_json::{Value, json};

const CARD: &str = "web-1";

fn f(file: &str, line: u32, summary: &str, proof: &str) -> Value {
    json!({"file": file, "line": line, "summary": summary, "proof": proof})
}

fn to_failed_round(s: &mut Sim, suite_exit: i32, findings: &[Value]) {
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(CARD, &[], findings);
    if !findings.is_empty() {
        s.proofs_copied();
    }
    s.suite(CARD, suite_exit);
}

#[test]
fn triage_validation_refuses_bad_input_and_wrong_sender() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "First bug", "p1"), f("src/b.rs", 9, "Second bug", "p2")]);
    assert!(s.card(CARD).triage.is_some());
    assert!(!s.owed(&format!("{CARD}/foreman")));

    assert!(s.operator(json!({"triage": {"card": CARD, "fix": ["1"]}})).is_err());
    assert!(s.operator(json!({"triage": {"card": CARD, "fix": ["1", "2"], "decline": ["2"]}})).is_err());
    assert!(s.operator(json!({"triage": {"card": CARD, "fix": ["1", "9"]}})).is_err());
    assert!(s.operator(json!({"triage": {"card": CARD, "fix": ["1", "abc"]}})).is_err());
    assert!(
        s.cmd(Sender::Agent(format!("{CARD}/foreman").into()), json!({"triage": {"card": CARD, "fix": ["1", "2"]}}))
            .is_err()
    );
    assert!(s.cmd(Sender::Orchestrator("web".into()), json!({"triage": {"card": CARD, "fix": ["1", "2"]}})).is_err());
}

#[test]
fn triage_offers_pause_resume_and_give() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "First bug", "p1"), f("src/b.rs", 9, "Second bug", "p2")]);
    let ask_id = s.card(CARD).triage.unwrap().ask;
    assert!(s.operator(json!({"ack": {"ask": ask_id}})).is_err());
    let sc = screen(&s.state, &Outside::default(), s.ctx());
    let hand = sc.hands.iter().find(|h| h.ask == ask_id).unwrap();
    assert_eq!(hand.kind, AskKind::Triage);
    let labels: Vec<&str> = hand.offers.iter().map(|o| o.label.as_str()).collect();
    assert!(labels.contains(&"Orchestrator answers") && labels.contains(&"Opinion from orchestrator"));
    assert!(!labels.contains(&"ack"));

    s.operator(json!({"stop": {"card": CARD}})).unwrap();
    assert!(s.card(CARD).triage.is_some(), "stop must keep triage open");
    assert!(s.operator(json!({"triage": {"card": CARD, "fix": ["1", "2"]}})).is_err());
    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    assert!(s.card(CARD).triage.is_some(), "resume must keep triage open");

    s.operator(json!({"give": {"ask": ask_id}})).unwrap();
    s.cmd(Sender::Orchestrator("web".into()), json!({"triage": {"card": CARD, "fix": ["1"], "foreman": ["2"]}}))
        .unwrap();
    assert!(s.card(CARD).triage.is_none());
}

#[test]
fn triage_all_declined_with_passing_suite_restarts_verification_without_foreman() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Style nit", "p1"), f("src/b.rs", 9, "Another nit", "p2")]);
    let rc = s.review_card(CARD);
    let reqs_before = s.engine_to(CARD, "foreman").len();

    s.operator(json!({"triage": {"card": CARD, "decline": ["1", "2"]}})).unwrap();
    assert!(s.card(CARD).triage.is_none());
    let raised = &s.card(&rc).review.unwrap().raised;
    assert!(raised.iter().all(|r| r.resolved && r.declined));
    assert_eq!(s.engine_to(CARD, "foreman").len(), reqs_before, "all-declined pass must not wake foreman");
    assert_eq!(s.stage(CARD), Stage::Merging, "checks stand so card goes straight back to verification");
}

fn suite_with_log(s: &mut Sim, card: &str, exit: i32, log_path: &str) {
    let pick = |p: &Purpose| {
        p.card().is_some_and(|c| c.as_str() == card)
            && matches!(p, Purpose::TrainRun { checks: false, unwalled: false, .. })
    };
    let id = s.open_jobs(pick)[0].clone();
    let Purpose::TrainRun { commit, .. } = s.purpose(&id) else { unreachable!() };
    let tree = match s.card(card).landing {
        Some(Landing::Test(k) | Landing::Review(k)) => k.tree,
        l => panic!("{card} isn't under test: {l:?}"),
    };
    let at = Checkout { head: Some(commit.clone()), tree, tracked_clean: true, clean: true };
    let logs = [(mork_types::CardId::new(card), log_path.to_owned())].into_iter().collect();
    let run = FinalRun {
        head: commit,
        before: at.clone(),
        after: at,
        checks: None,
        ran: true,
        exit: Some(exit),
        output: format!("SUITE-OUTPUT exit {exit}\nThe full log: /tmp/raw.log"),
        phase: Some("walled".into()),
        failing: None,
        logs,
    };
    s.end_job(&id, json!(run));
    s.settle_actions();
}

#[test]
fn triage_mixed_and_suite_failure_send_one_foreman_request() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(
        CARD,
        &[],
        &[
            f("src/a.rs", 3, "Fix this", "p1"),
            f("src/b.rs", 9, "Skip this", "p2"),
            f("src/c.rs", 12, "Weigh this", "p3"),
        ],
    );
    s.proofs_copied();
    suite_with_log(&mut s, CARD, 1, ".git/mork/suite-logs/run-1.log");
    assert!(s.card(CARD).triage.as_ref().unwrap().suite_failure.is_some());
    assert!(!s.owed(&format!("{CARD}/foreman")), "suite failure must wait for triage");

    let reqs_before = s.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).count();
    s.operator(json!({"triage": {"card": CARD, "fix": ["1"], "decline": ["2"], "foreman": ["3"]}})).unwrap();
    s.imported(CARD);
    let reqs: Vec<_> = s.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).collect();
    assert_eq!(reqs.len(), reqs_before + 1, "mixed triage + suite failure must send exactly one request");
    let last = reqs.last().unwrap();
    assert!(last.text.contains("1. (#1) [fix] src/a.rs:3: Fix this"), "{}", last.text);
    assert!(last.text.contains("3. (#3) [yours to decide] src/c.rs:12: Weigh this"), "{}", last.text);
    assert!(last.text.contains("Declined and already closed: #2"), "{}", last.text);
    assert!(!last.text.contains("Skip this"), "declined finding must not be in the foreman's list: {}", last.text);
    assert!(last.text.contains("SUITE-OUTPUT exit 1"), "suite failure must travel in the same request: {}", last.text);
    assert!(
        last.text.contains("The full log: .git/mork/suite-logs/run-1.log"),
        "suite log pointer must travel in the same request: {}",
        last.text
    );

    // All declined with a failed suite sends the suite failure alone in one foreman request.
    let mut s2 = Sim::new();
    to_failed_round(&mut s2, 1, &[f("src/a.rs", 3, "Nit", "p1")]);
    let reqs2_before = s2.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).count();
    s2.operator(json!({"triage": {"card": CARD, "decline": ["1"]}})).unwrap();
    s2.imported(CARD);
    let reqs2: Vec<_> = s2.engine_to(CARD, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).collect();
    assert_eq!(reqs2.len(), reqs2_before + 1);
    let req2 = reqs2.last().unwrap();
    assert!(
        !req2.text.contains("Review round"),
        "all-declined + failed suite must send suite failure alone: {}",
        req2.text
    );
    assert!(req2.text.contains("SUITE-OUTPUT exit 1"), "{}", req2.text);
}

#[test]
fn triage_excludes_pre_resolved_from_record_and_reply() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Bug 1", "p1"), f("src/b.rs", 9, "Bug 2", "p2")]);
    let rc = s.review_card(CARD);
    s.operator(json!({"resolve": {"card": rc, "n": 2}})).unwrap();
    let ans = s.operator(json!({"triage": {"card": CARD, "fix": ["1"], "decline": ["2"]}})).unwrap();
    assert_eq!(ans["triage"]["fix"], json!([1]));
    assert_eq!(ans["triage"]["decline"], json!([]));
}

#[test]
fn triage_withdraws_on_built_brief_mock_and_close() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Bug", "p1")]);
    let ask_id = s.card(CARD).triage.unwrap().ask;
    s.operator(json!({"say": {"place": CARD, "text": "rebuild"}})).unwrap();
    s.built(CARD, "h2", "base1");
    assert!(s.card(CARD).triage.is_none());
    assert!(!s.state.asks[&ask_id].open);

    let mut s2 = Sim::new();
    to_failed_round(&mut s2, 0, &[f("src/a.rs", 3, "Bug", "p1")]);
    let ask2 = s2.card(CARD).triage.unwrap().ask;
    s2.operator(json!({"brief": {"card": CARD}})).unwrap();
    assert!(s2.card(CARD).triage.is_none());
    assert!(!s2.state.asks[&ask2].open);

    let mut s3 = Sim::new();
    to_failed_round(&mut s3, 0, &[f("src/a.rs", 3, "Bug", "p1")]);
    let ask3 = s3.card(CARD).triage.unwrap().ask;
    s3.operator(json!({"close": {"card": CARD, "why": "not needed"}})).unwrap();
    assert!(s3.card(CARD).triage.is_none());
    assert!(!s3.state.asks[&ask3].open);
}

#[test]
fn views_fold_branch_review_and_map_hands_to_parent() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Race condition", "proof text")]);
    let rc = s.review_card(CARD);

    let sc = screen(&s.state, &Outside::default(), s.ctx());
    let board = sc.boards.iter().find(|b| b.project.as_str() == "web").unwrap();
    assert!(board.live.cards.iter().any(|r| r.id.as_str() == CARD));
    assert!(!board.live.cards.iter().any(|r| r.id.as_str() == rc), "folded review must not appear on board");
    let rc_row = sc.rooms.iter().find(|r| r.place.as_str() == rc).expect("folded review room stays under parent");
    assert_eq!(rc_row.under.as_ref().map(|u| u.card.as_str()), Some(CARD));

    let parent_row = board.live.cards.iter().find(|r| r.id.as_str() == CARD).unwrap();
    let live = parent_row.live.as_ref().unwrap();
    assert_eq!(live.stage, "Verify · Round 1");
    assert_eq!(live.light, Some(Color::Yellow));

    let rpv = place(&s.state, &rc, None, &Outside::default(), s.ctx());
    assert_eq!(rpv.card.as_ref().unwrap().parent.as_ref().map(|c| c.as_str()), Some(CARD));

    s.operator(json!({"say": {"place": rc, "text": "ping"}})).unwrap();
    let mut ask_reply = reply("working", "Question");
    ask_reply["hand"]["message"] = json!("Keep going?");
    ask_reply["hand"]["ask"] = json!(true);
    s.turn(&format!("{rc}/foreman"), ask_reply);
    let sc2 = screen(&s.state, &Outside::default(), s.ctx());
    let rev_hand = sc2.hands.iter().find(|h| h.place.as_str() == rc).unwrap();
    assert_eq!(rev_hand.card.as_ref().map(|c| c.as_str()), Some(CARD));
}

#[test]
fn views_expose_four_step_pipeline_rounds_and_triage() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Race condition", "proof text")]);
    let rc = s.review_card(CARD);

    let pv = place(&s.state, CARD, None, &Outside::default(), s.ctx());
    let header = pv.card.as_ref().unwrap();
    let labels: Vec<&str> = header.pipeline.iter().map(|m| m.label.as_str()).collect();
    assert_eq!((header.parent.as_ref(), labels), (None, vec!["Spec", "Build", "Verify", "Land"]));
    let v = &header.pipeline[2];
    assert_eq!(
        (v.kind, v.at, v.count.as_deref(), v.sub.as_deref()),
        (
            mork_types::view::StageKind::Verify,
            StageAt::Current,
            Some("Round 1"),
            Some("failed · 1 finding waits for you"),
        )
    );

    let review = pv.review.as_ref().expect("folded review seats");
    assert_eq!(
        (review.card.as_str(), review.round, review.foreman.as_ref().map(|f| f.agent.as_str())),
        (rc.as_str(), 1, Some(format!("{rc}/foreman").as_str()))
    );

    let rounds: Vec<_> = pv.overview.as_ref().unwrap().stages.iter().filter_map(|st| st.round.as_ref()).collect();
    assert_eq!(rounds.len(), 1);
    let f0 = &rounds[0].findings[0];
    assert_eq!(
        (rounds[0].verdict.as_str(), f0.summary.as_str(), f0.proof.as_str()),
        ("failed", "Race condition", "proof text")
    );

    let triage = pv.triage.as_ref().expect("open triage view");
    assert_eq!((triage.round, triage.findings[0].proof.as_str()), (1, "proof text"));

    // Spec room must not serialize the build room's triage payload (#17).
    let spec_pv = place(&s.state, &format!("{CARD}/spec"), None, &Outside::default(), s.ctx());
    assert!(spec_pv.triage.is_none());
}

#[test]
fn timeline_splits_merging_into_verify_and_land_across_pause_and_reverify() {
    use mork_core::views::timeline::{points, totals};
    let rows = [
        (0, "card.stage", json!({"stage": "merging", "paused": false})),
        (100, "merge.tested", json!({})),
        (150, "card.stage", json!({"stage": "merging", "paused": true})),
        (200, "card.stage", json!({"stage": "merging", "paused": false})),
        (260, "merge.reverify", json!({})),
        (300, "card.landed", json!({})),
    ];
    let borrowed: Vec<(i64, &str, &Value)> = rows.iter().map(|(t, k, v)| (*t, *k, v)).collect();
    let (pts, _) = points(&borrowed);
    let tot = totals(&pts, 400);
    assert_eq!(tot.stages.get("verify").copied(), Some(140), "{:?}", tot.stages);
    assert_eq!(tot.stages.get("land").copied(), Some(110), "{:?}", tot.stages);
    assert_eq!(tot.paused, 50);
}

fn timeline_outside(s: &Sim) -> Outside {
    let rows: Vec<(i64, &str, &Value)> = s
        .journal
        .iter()
        .filter(|(_, _, c, _)| c.as_deref() == Some(CARD))
        .map(|(t, k, _, d)| (*t, k.as_str(), d))
        .collect();
    let from = rows.iter().rposition(|(_, k, _)| *k == "card.start").unwrap_or(0);
    let mut o = Outside::default();
    o.timelines.insert(CARD.into(), mork_core::views::timeline::points(&rows[from..]).0);
    o
}

fn overview_labels(o: &mork_types::view::OverviewView) -> Vec<&str> {
    o.stages.iter().map(|st| st.label.as_str()).collect()
}

#[test]
fn overview_stages_carry_a_failed_round_and_its_successor() {
    use mork_types::view::OverviewStageAt;

    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Bug 1", "p1"), f("src/b.rs", 9, "Bug 2", "p2")]);

    let pv = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx());
    let ov = pv.overview.as_ref().expect("card room carries overview");
    assert_eq!(overview_labels(ov), vec!["Spec", "Build", "Verify · Round 1"]);
    assert_eq!(ov.stages[2].at, OverviewStageAt::Current);
    assert_eq!(ov.stages[2].summary, "2 to triage");
    assert!(place(&s.state, &format!("{CARD}/spec"), Some(100), &timeline_outside(&s), s.ctx()).overview.is_none());

    s.operator(json!({"triage": {"card": CARD, "fix": ["1"], "decline": ["2"]}})).unwrap();
    s.imported(CARD);
    s.built(CARD, "h2", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);

    let pv2 = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx());
    let ov2 = pv2.overview.as_ref().unwrap();
    assert_eq!(overview_labels(ov2), vec!["Spec", "Build", "Verify · Round 1", "Verify · Round 2"]);
    assert_eq!(ov2.stages[2].at, OverviewStageAt::Failed);
    assert_eq!(ov2.stages[2].summary, "1 declined, 1 open");
}

#[test]
fn overview_checks_failure_unasked_round_and_restarted_review_run() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    let ov_checks = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_checks), vec!["Spec", "Build"]);

    s.checks(CARD, 1, &[f("src/a.rs", 1, "fmt", "diff")]);
    let ov_back = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_back), vec!["Spec", "Build"]);

    s.built(CARD, "h2", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(CARD, &[], &[f("src/a.rs", 3, "Bug 1", "p1")]);
    s.proofs_copied();
    s.suite(CARD, 0);
    s.operator(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    s.imported(CARD);
    s.built(CARD, "h3", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);

    let ov_unasked = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_unasked), vec!["Spec", "Build", "Verify · Round 1", "Verify · Round 2"]);

    // Stopping and resuming while R2 is unasked must not shift R2's start.
    s.operator(json!({"stop": {"card": CARD}})).unwrap();
    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    let ov_paused = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(ov_paused.stages[3].start_ms, ov_unasked.stages[3].start_ms);

    s.operator(json!({"close": {"card": CARD, "why": "redo"}})).unwrap();
    for id in s.open_jobs(|p| matches!(p, Purpose::Bundle { .. })) {
        s.end_job(&id, json!({"branch": CARD, "bundle": false, "head": null, "clean": true, "dirty": []}));
    }
    s.settle_actions();
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    let ov_restart = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_restart), vec!["Spec"]);
}

#[test]
fn overview_pause_in_land_and_reopened_standalone_review_drop_prior_rounds() {
    use mork_types::card::{Card, Mode};
    use mork_types::review::{Review, Round, Verdict};
    use mork_types::{CardId, Sha};

    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    s.round_asked(CARD);
    s.round_ends(CARD, &[], &[]);
    s.suite(CARD, 0);
    s.operator(json!({"stop": {"card": CARD}})).unwrap();
    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    let ov_land = place(&s.state, CARD, Some(100), &timeline_outside(&s), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_land), vec!["Spec", "Build", "Verify · Round 1", "Land"]);

    // A reopened standalone review card carrying run 1's finished round must not show R1 as run 2's milestone.
    let rc_id = CardId::new("pr-web-9");
    let mut rc = Card::new(rc_id.clone(), "web", "PR 9", Mode::Review, 2, 200);
    rc.stage = Stage::Review;
    rc.first_seq = 50;
    rc.review = Some(Box::new(Review {
        rounds: vec![Round {
            test_changes: Vec::new(),
            n: 1,
            head: None,
            commit: Sha::new("aaa"),
            base: Sha::new("bbb"),
            files: Default::default(),
            carried: Default::default(),
            asked: Some(10),
            verdict: Some(Verdict::Passed),
            proofs: None,
            submitted: None,
            named: Vec::new(),
            reviewers: Default::default(),
        }],
        ..Review::default()
    }));
    s.state.cards.insert(rc_id.clone(), rc);
    let ov_pr = place(&s.state, rc_id.as_str(), Some(100), &Outside::default(), s.ctx()).overview.unwrap();
    assert_eq!(overview_labels(&ov_pr), vec!["Review"]);
}

#[test]
fn verify_step_names_the_orchestrator_when_triage_is_given() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 0, &[f("src/a.rs", 3, "Race condition", "proof text")]);
    let ask = s.card(CARD).triage.unwrap().ask;
    s.operator(json!({"give": {"ask": ask}})).unwrap();
    let pv = place(&s.state, CARD, None, &Outside::default(), s.ctx());
    let verify = &pv.card.as_ref().unwrap().pipeline[2];
    assert_eq!(verify.sub.as_deref(), Some("failed · 1 finding waits for orchestrator"));
}

#[test]
fn overview_live_rows_keep_the_run_through_triage_and_show_the_foreman_fixing() {
    let mut s = Sim::new();
    to_failed_round(&mut s, 1, &[f("src/a.rs", 3, "Race condition", "proof text")]);
    let live = |s: &Sim| -> Vec<(String, String)> {
        let pv = place(&s.state, CARD, None, &timeline_outside(s), s.ctx());
        pv.overview.unwrap().live.into_iter().map(|r| (r.name, r.words)).collect()
    };
    let rows = live(&s);
    assert_eq!(rows[..2], [("Checks".into(), "passed".into()), ("Suite".into(), "failed".into())], "{rows:?}");

    s.operator(json!({"triage": {"card": CARD, "fix": ["1"]}})).unwrap();
    s.imported(CARD);
    s.turn_started(&format!("{CARD}/foreman"));
    assert_eq!(live(&s).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec!["foreman"]);
}

#[test]
fn suite_row_counts_a_resumed_phases_newest_attempt_alone() {
    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    s.built(CARD, "h1", "base1");
    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    let suite = |p: &Purpose| matches!(p, Purpose::TrainRun { checks: false, .. });
    let first = s.open_jobs(suite)[0].clone();
    s.apply(Event::JobRunning { id: first.clone() });
    s.apply(Event::JobEnded { id: first.clone(), end: JobEnd::Cut { session: SessionSeen::default() } });
    s.settle_actions();
    s.operator(json!({"resume": {"card": CARD}})).unwrap();
    s.settle_actions();
    let retry = s.open_jobs(suite)[0].clone();
    assert_ne!(first, retry);
    let mut o = timeline_outside(&s);
    o.suites.insert(first, SuiteProgress { passed: 30, total: Some(100), ..SuiteProgress::default() });
    o.suites.insert(retry, SuiteProgress { passed: 2, total: Some(100), ..SuiteProgress::default() });
    let pv = place(&s.state, CARD, None, &o, s.ctx());
    let row = pv.overview.unwrap().live.into_iter().find(|r| r.name == "Suite").unwrap();
    assert_eq!(row.progress.map(|p| (p.passed, p.total)), Some((2, Some(100))), "{}", row.words);
}

#[test]
fn board_live_row_progress_tracks_steps_plan_suite_and_triage() {
    let progress_of = |s: &Sim, outside: &Outside| -> u8 {
        let sc = screen(&s.state, outside, s.ctx());
        let board = sc.boards.iter().find(|b| b.project.as_str() == "web").unwrap();
        board.live.cards.iter().find(|r| r.id.as_str() == CARD).unwrap().live.as_ref().unwrap().progress
    };

    let mut s = Sim::new();
    s.board(CARD, "web");
    s.operator(json!({"start": {"card": CARD}})).unwrap();
    s.settle_actions();
    assert_eq!(progress_of(&s, &Outside::default()), 0, "Spec has no within-step progress");

    let mut s = Sim::new();
    s.through_brief(CARD, "web");
    assert_eq!(progress_of(&s, &Outside::default()), 25, "Build without plan starts at 25%");

    let foreman = Sender::Agent(format!("{CARD}/foreman").into());
    s.cmd(foreman.clone(), json!({"deliverables": {"card": CARD, "names": "ui"}})).unwrap();
    s.cmd(
        foreman.clone(),
        json!({"plan": {"card": CARD, "deliverable": "ui", "tasks": "t1\nt2\nt3\nt4\nt5\nt6\nt7\nt8"}}),
    )
    .unwrap();
    s.cmd(foreman, json!({"plan.task": {"card": CARD, "deliverable": "ui", "task": "4"}})).unwrap();
    assert_eq!(progress_of(&s, &Outside::default()), 34, "Build 3/8 done = floor(100 * 1.375 / 4)");

    s.built(CARD, "h1", "base1");
    assert_eq!(progress_of(&s, &Outside::default()), 50, "Verify checks before suite starts");

    s.checks(CARD, 0, &[]);
    s.candidate(CARD);
    let suite_job = s.open_jobs(|p| matches!(p, Purpose::TrainRun { checks: false, .. }))[0].clone();
    let mut running = Outside::default();
    running.suites.insert(
        suite_job.clone(),
        SuiteProgress { passed: 30, failed: 20, total: Some(100), ..SuiteProgress::default() },
    );
    assert_eq!(progress_of(&s, &running), 62, "Verify suite 50/100 = floor(100 * 2.5 / 4)");

    let mut clamped = Outside::default();
    clamped.suites.insert(suite_job, SuiteProgress { passed: 120, total: Some(100), ..SuiteProgress::default() });
    assert_eq!(progress_of(&s, &clamped), 75, "Verify suite clamps to 100% of Verify step");

    let mut s_triage = Sim::new();
    to_failed_round(&mut s_triage, 0, &[f("src/a.rs", 3, "Bug", "p1")]);
    assert_eq!(progress_of(&s_triage, &Outside::default()), 75, "Failed round waiting for triage counts suite ended");
}
