// fac-570: a review round is not superseded by its own commit or head, so a reviewer's resolve in that round succeeds.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, contents, reply, sha};
use mork_core::decide::{Event, JobEnd, SessionSeen};
use mork_types::action::ActionWhat;
use mork_types::card::{Landing, Stage};
use mork_types::review::{Review, Verdict};
use mork_types::state::Sender;
use mork_types::work::{CheckRun, Checkout, FinalRun, Made, OpenPr, Purpose};
use mork_types::{AgentKey, CardId, JobId, Sha};
use serde_json::{Value, json};

fn open_pr(pr: u64, head: &Sha) -> OpenPr {
    OpenPr {
        review_requested: false,
        pr,
        author: "colleague".into(),
        title: format!("PR {pr}"),
        branch: format!("feat-{pr}"),
        head: head.clone(),
        ours: false,
        dest: "master".into(),
        updated: String::new(),
        link: String::new(),
        description: String::new(),
        tickets: Vec::new(),
    }
}

fn finding(file: &str, line: i64, summary: &str) -> Value {
    json!({"file": file, "line": line, "summary": summary, "proof": "proof"})
}

fn verdict_reply(outcome: &str, headline: &str, findings: Vec<Value>) -> Value {
    let mut r = reply(outcome, headline);
    r["findings"] = json!(findings);
    r
}

fn finish_turn(sim: &mut Sim, id: JobId, reply: Value) {
    let session = SessionSeen { started: true, ..SessionSeen::default() };
    sim.apply(Event::JobEnded { id, end: JobEnd::Finished { reply: Ok(reply), session } });
}

fn settle_fetch(sim: &mut Sim, project: &str, branch: &str, head: &Sha, base: &str) {
    let fetches: Vec<_> = sim
        .pending()
        .into_iter()
        .filter(|a| matches!(&a.what, ActionWhat::Fetch { project: p, .. } if p == project))
        .collect();
    for a in fetches {
        let base_sha = sha(base);
        let result = json!({"base_tip": base_sha, "tips": {"master": base_sha, branch: head}});
        sim.returned(&a.id, Ok(result));
    }
}

fn pass_train_checks(sim: &mut Sim, card: &str, commit: &Sha, tree: &str) {
    let check = sim.open_jobs(|p| matches!(p, Purpose::TrainRun { card: c, checks: true, .. } if c.as_str() == card))
        [0]
    .clone();
    let at = Checkout { head: Some(commit.clone()), tree: Some(tree.into()), tracked_clean: true, clean: true };
    let run = FinalRun {
        head: commit.clone(),
        before: at.clone(),
        after: at,
        checks: Some(CheckRun {
            head: commit.clone(),
            exit: Some(0),
            report: Some(json!({"status": 0, "findings": [], "base": sha("base1"), "head": commit})),
            folder_commit: "c1".into(),
            folder_dirty: false,
            located: Vec::new(),
            folder_moved: false,
        }),
        ran: true,
        exit: Some(0),
        output: String::new(),
        phase: None,
        failing: None,
        logs: Default::default(),
    };
    sim.end_job(&check, json!(run));
}

fn drive_entry_to_round(sim: &mut Sim, rc: &str, commit: &Sha, files: &[&str]) {
    let id = sim.open_jobs(|p| matches!(p, Purpose::Candidate { card, .. } if card.as_str() == rc))[0].clone();
    let tree = format!("tree-{commit}");
    let made = Made {
        commit: Some(commit.clone()),
        tree: Some(tree.clone()),
        contents: contents(commit.as_str(), files),
        conflicts: Vec::new(),
        into_pushed: false,
        review_changes: Some(contents(commit.as_str(), files)),
    };
    sim.end_job(&id, serde_json::to_value(made).unwrap());
    pass_train_checks(sim, rc, commit, &tree);
    let clone = sim.open_jobs(|p| matches!(p, Purpose::Clone { card, .. } if card.as_str() == rc))[0].clone();
    sim.end_job(&clone, json!({"at": commit}));
}

fn fail_entry_round_one(sim: &mut Sim, rc: &str, h1: &Sha, c1: &Sha) {
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, h1)] });
    settle_fetch(sim, "ex", "feat-5603", h1, "base1");
    drive_entry_to_round(sim, rc, c1, &["src/a.rs"]);
    sim.turn_sending(
        &format!("{rc}/foreman"),
        &[json!({"spawn": {"card": rc, "role": "reviewer", "topic": "correctness", "task": "Check"}})],
        reply("working", "Spawned"),
    );
    sim.turn(
        &format!("{rc}/reviewer-1"),
        verdict_reply("review-fail", "Bug", vec![finding("src/a.rs", 10, "bug in a")]),
    );
    sim.turn(
        &format!("{rc}/foreman"),
        verdict_reply("review-fail", "Failed", vec![finding("src/a.rs", 10, "[reviewer-1] bug in a")]),
    );
    sim.suite(rc, 0);
    sim.settle_actions();
}

#[test]
fn pr_entry_round_not_superseded_by_own_commit_and_resolve_succeeds() {
    let mut sim = Sim::new();
    let (h1, c1, h2, c2, h3) = (sha("h1"), sha("c1"), sha("h2"), sha("c2"), sha("h3"));
    let rc = "pr-ex-5603";
    fail_entry_round_one(&mut sim, rc, &h1, &c1);

    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &h2)] });
    settle_fetch(&mut sim, "ex", "feat-5603", &h2, "base1");
    drive_entry_to_round(&mut sim, rc, &c2, &["src/a.rs"]);
    sim.turn_sending(
        &format!("{rc}/foreman"),
        &[json!({"say": {"place": rc, "to": "reviewer-1", "text": "Verify #1", "kind": "request"}})],
        reply("working", "Asked"),
    );
    let r2_job = sim.turn_started(&format!("{rc}/reviewer-1"));

    // Poller reports c2 (round 2's own commit, while card.head is h2): keeps round 2 and syncs head to c2.
    let superseded_before = sim.journal.iter().filter(|(_, k, _, _)| k == "review.superseded").count();
    let moved_before = sim.journal.iter().filter(|(_, k, _, _)| k == "entry.moved").count();
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &c2)] });
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "review.superseded").count(), superseded_before);
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "entry.moved").count(), moved_before);
    let card = sim.card(rc);
    assert_eq!(card.head.as_ref(), Some(&c2));
    assert!(card.landing.is_some());
    assert!(card.review.as_deref().and_then(Review::open).is_some_and(|o| o.n == 2 && o.head.as_ref() == Some(&c2)));

    // Reviewer-1's resolve in round 2 succeeds, and finishing records Finished { head: c2 } with no repeat round.
    let ans =
        sim.cmd(Sender::Agent(AgentKey::new(format!("{rc}/reviewer-1"))), json!({"resolve": {"card": rc, "n": 1}}));
    assert!(ans.is_ok(), "resolve in round 2 must succeed: {ans:?}");
    finish_turn(&mut sim, r2_job, reply("review-pass", "Fixed"));
    sim.turn(&format!("{rc}/foreman"), reply("review-pass", "Clean"));
    sim.suite(rc, 0);
    sim.settle_actions();
    assert_eq!(sim.card(rc).review.unwrap().finished.unwrap().head, c2);

    // Subsequent poll of c2 does not restart the entry, whereas a genuinely new head h3 does.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &c2)] });
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "entry.moved").count(), moved_before);
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &h3)] });
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "entry.moved").count(), moved_before + 1);
}

fn setup_linked_round_two(sim: &mut Sim, card: &str, c2: &Sha) -> (String, JobId) {
    sim.through_brief(card, "web");
    sim.built(card, "h1", "base1");
    sim.checks(card, 0, &[]);
    sim.candidate(card);
    sim.round_asked(card);
    let rc = sim.review_card(card);
    sim.turn_sending(
        &format!("{rc}/foreman"),
        &[json!({"spawn": {"card": rc, "role": "reviewer", "topic": "correctness", "task": "Check"}})],
        reply("working", "Spawned"),
    );
    sim.turn(
        &format!("{rc}/reviewer-1"),
        verdict_reply("review-fail", "Bug", vec![finding("src/a.rs", 10, "bug in a")]),
    );
    sim.round_ends(card, &[], &[finding("src/a.rs", 10, "[reviewer-1] bug in a")]);
    sim.proofs_copied();
    sim.suite(card, 0);
    sim.operator(json!({"triage": {"card": card, "fix": ["1"], "decline": [], "foreman": []}})).unwrap();
    sim.settle_actions();
    sim.imported(card);

    sim.built(card, "h2", "base1");
    sim.checks(card, 0, &[]);
    let cand_id = sim.open_jobs(|p| matches!(p, Purpose::Candidate { card: c, .. } if c.as_str() == card))[0].clone();
    let tree = format!("tree-{c2}");
    let made = Made {
        commit: Some(c2.clone()),
        tree: Some(tree.clone()),
        contents: contents("h2", &["src/a.rs"]),
        conflicts: Vec::new(),
        into_pushed: false,
        review_changes: None,
    };
    sim.end_job(&cand_id, serde_json::to_value(made).unwrap());
    pass_train_checks(sim, card, c2, &tree);
    sim.round_asked(card);
    sim.turn_sending(
        &format!("{rc}/foreman"),
        &[json!({"say": {"place": rc, "to": "reviewer-1", "text": "Verify #1", "kind": "request"}})],
        reply("working", "Asked"),
    );
    let r2_job = sim.turn_started(&format!("{rc}/reviewer-1"));
    (rc, r2_job)
}

#[test]
fn linked_branch_round_not_superseded_by_own_commit_and_resolve_succeeds() {
    let mut sim = Sim::new();
    let card = "web-1";
    let c2 = sha("c2");
    let (rc, r2_job) = setup_linked_round_two(&mut sim, card, &c2);

    // Taking round 2's own candidate commit c2 as head keeps verification and round 2 intact without replacement checks.
    let superseded_before = sim.journal.iter().filter(|(_, k, _, _)| k == "review.superseded").count();
    sim.operator(json!({"say": {"place": card, "to": "foreman", "text": "ping", "kind": "request"}})).unwrap();
    sim.built(card, "c2", "base1");
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "review.superseded").count(), superseded_before);
    assert_eq!(sim.card(card).stage, Stage::Merging);
    assert!(sim.card(card).landing.is_some());
    assert!(sim.open_jobs(|p| matches!(p, Purpose::Check { card: c, .. } if c.as_str() == card)).is_empty());
    assert!(
        sim.card(&rc)
            .review
            .as_deref()
            .and_then(Review::open)
            .is_some_and(|o| o.n == 2 && o.submitted.as_ref() == Some(&c2))
    );

    let ans =
        sim.cmd(Sender::Agent(AgentKey::new(format!("{rc}/reviewer-1"))), json!({"resolve": {"card": rc, "n": 1}}));
    assert!(ans.is_ok(), "resolve in round 2 must succeed: {ans:?}");
    assert!(sim.card(&rc).review.unwrap().raised[0].resolved);
    finish_turn(&mut sim, r2_job, reply("review-pass", "Fixed"));

    // A genuinely different head h3 still supersedes round 2.
    sim.operator(json!({"say": {"place": card, "to": "foreman", "text": "fix", "kind": "request"}})).unwrap();
    sim.built(card, "h3", "base1");
    assert_eq!(sim.journal.iter().filter(|(_, k, _, _)| k == "review.superseded").count(), superseded_before + 1);
    assert_eq!(sim.card(&rc).review.unwrap().rounds[1].verdict, Some(Verdict::Superseded));
}

#[test]
fn adopt_and_branch_repeat_still_supersede() {
    let mut sim = Sim::new();
    let head = sha("h1");
    let rc = "pr-ex-7";

    // Start a live PR entry reviewing head h1 (candidate commit also h1).
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(7, &head)] });
    settle_fetch(&mut sim, "ex", "feat-7", &head, "base1");
    drive_entry_to_round(&mut sim, rc, &head, &["src/a.rs"]);

    // Our full card ex-1 owns PR 7 and enters verification on the same head h1: adoption must supersede round 1.
    sim.through_brief("ex-1", "ex");
    let mut ours = sim.card("ex-1");
    ours.pr = Some(7);
    sim.state.cards.insert(CardId::new("ex-1"), ours);
    sim.built("ex-1", "h1", "base1");
    sim.checks("ex-1", 0, &[]);
    sim.candidate("ex-1");

    let adopted = sim.card(rc);
    assert_eq!(adopted.stage, Stage::Review);
    assert!(adopted.landing.is_none());
    let review = adopted.review.unwrap();
    assert_eq!(review.of, Some(CardId::new("ex-1")));
    assert_eq!(review.rounds[0].verdict, Some(Verdict::Superseded));
    assert!(review.open().is_some_and(|o| o.n == 2));

    // The operator's `review.branch` repeat (head = None) still supersedes an open round.
    sim.operator(json!({"review.branch": {"project": "web", "branch": "colleague-br"}})).unwrap();
    settle_fetch(&mut sim, "web", "colleague-br", &head, "base1");
    let brc = "branch-web-colleague-br";
    drive_entry_to_round(&mut sim, brc, &head, &["src/a.rs"]);
    sim.operator(json!({"review.branch": {"project": "web", "branch": "colleague-br"}})).unwrap();
    assert_eq!(sim.card(brc).review.unwrap().rounds[0].verdict, Some(Verdict::Superseded));
}

#[test]
fn sync_open_head_skips_while_prepare_runs() {
    let mut sim = Sim::new();
    let (h1, c1, h2, c2) = (sha("h1"), sha("c1"), sha("h2"), sha("c2"));
    let rc = "pr-ex-5603";
    fail_entry_round_one(&mut sim, rc, &h1, &c1);

    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &h2)] });
    settle_fetch(&mut sim, "ex", "feat-5603", &h2, "base1");
    drive_entry_to_round(&mut sim, rc, &c2, &["src/a.rs"]);

    // Stop and resume so a new Candidate job for h2 is running in Landing::Prepare while round 2 stays open on c2.
    sim.suite(rc, 0);
    sim.operator(json!({"stop": {"card": rc}})).unwrap();
    sim.operator(json!({"resume": {"card": rc}})).unwrap();
    settle_fetch(&mut sim, "ex", "feat-5603", &h2, "base1");
    assert!(matches!(sim.card(rc).landing, Some(Landing::Prepare(_))));

    // Polling c2 while Prepare runs must not rewrite Card.head and stale the in-flight Candidate job.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &c2)] });
    assert_eq!(sim.card(rc).head.as_ref(), Some(&h2));

    let cand_id = sim.open_jobs(|p| matches!(p, Purpose::Candidate { card, .. } if card.as_str() == rc))[0].clone();
    let made = Made {
        commit: Some(c2.clone()),
        tree: Some(format!("tree-{c2}-rebuilt")),
        contents: contents(c2.as_str(), &["src/a.rs"]),
        conflicts: Vec::new(),
        into_pushed: false,
        review_changes: Some(contents(c2.as_str(), &["src/a.rs"])),
    };
    sim.end_job(&cand_id, serde_json::to_value(made).unwrap());
    assert!(matches!(sim.card(rc).landing, Some(Landing::Test(_))), "Prepare job must advance to Test");

    // Once past Prepare, polling c2 syncs Card.head and the open round's source head to c2.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &c2)] });
    assert_eq!(sim.card(rc).head.as_ref(), Some(&c2));
    assert!(sim.card(rc).review.as_deref().and_then(Review::open).is_some_and(|o| o.head.as_ref() == Some(&c2)));
}

#[test]
fn a_secret_stops_a_pr_entry_before_its_round() {
    let mut sim = Sim::new();
    let head = sha("h1");
    let rc = "pr-ex-5603";
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(5603, &head)] });
    settle_fetch(&mut sim, "ex", "feat-5603", &head, "base1");
    sim.candidate(rc);
    let checks = |p: &Purpose| matches!(p, Purpose::TrainRun { card, checks: true, .. } if card.as_str() == rc);
    let jobs = sim.open_jobs(checks);
    let purpose = sim.purpose(&jobs[0]);
    let hit = "a credential, in fixture:2 (commit h1)";
    sim.answer_job("candidate checks", checks, json!({"secret_hit": hit}));
    assert!(
        sim.state.asks.values().any(|a| {
            a.open
                && a.card.as_deref() == Some(rc)
                && a.kind == mork_types::ask::AskKind::Checks
                && a.text.contains(hit)
        }),
        "the PR's checks notice must identify the detected credential and its location"
    );
    let card = sim.card(rc);
    assert_eq!(card.failed_job.as_ref(), Some(&purpose));
    assert!(card.review.as_ref().unwrap().rounds.is_empty());
    assert!(sim.open_jobs(|p| matches!(p, Purpose::TrainRun { checks: false, .. })).is_empty());
}
