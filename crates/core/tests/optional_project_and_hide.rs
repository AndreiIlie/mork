// fac-725: optional `[mork] project` and `[internals] hide` default-off behaviour.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Sim, reply, sha};
use mork_core::decide::{Event, Intent, JobEnd, SessionSeen};
use mork_core::internals::internals;
use mork_types::action::ActionWhat;
use mork_types::card::{Card, Stage};
use mork_types::config::Internals;
use mork_types::note::NoteWhat;
use mork_types::reply::closing_schema;
use mork_types::state::Sender;
use mork_types::work::{FinalRun, Purpose};
use mork_types::{AgentKey, CardId, ProjectId};
use serde_json::{Value, json};

fn finish_with_tools(s: &mut Sim, id: &mork_types::JobId, reply: Value) {
    let session = SessionSeen { started: true, tools: true, answered: true, ..SessionSeen::default() };
    s.apply(Event::JobEnded { id: id.clone(), end: JobEnd::Finished { reply: Ok(reply), session } });
}

fn sim_with_mork_project_unset() -> Sim {
    let mut s = Sim::new();
    // Register a `mork` project matching the default self-build name, then unset `[mork] project`.
    let fac = s.config.projects.get(&ProjectId::new("web")).unwrap().clone();
    s.config.projects.insert(ProjectId::new("mork"), fac);
    s.config.mork.project = None;
    s
}

#[test]
fn unset_project_skips_self_build_and_unwalled_phase() {
    let mut s = sim_with_mork_project_unset();
    s.through_brief("fac-1", "mork");
    s.built("fac-1", "h1", "base1");
    s.checks("fac-1", 0, &[]);
    s.candidate("fac-1");
    s.round_asked("fac-1");
    s.round_ends("fac-1", &[], &[]);
    s.suite("fac-1", 0);

    // No unwalled second phase was scheduled even for a card on project `mork`.
    assert!(
        !s.open_jobs(|p| matches!(p, Purpose::TrainRun { unwalled: true, .. })).into_iter().next().is_some(),
        "unset project owes no unwalled phase"
    );

    // Local MoveBase completes and lands the card with no Purpose::Build scheduled.
    let mv = s.pending().into_iter().find(|a| matches!(a.what, ActionWhat::MoveBase { .. })).unwrap();
    s.intents.clear();
    s.returned(&mv.id, Ok(json!({"after": sha("h1")})));
    assert_eq!(s.stage("fac-1"), Stage::Done);
    assert!(
        !s.intents.iter().any(|i| matches!(i, Intent::StartJob { purpose: Purpose::Build { .. } })),
        "unset project starts no self-build on landing"
    );
}

#[test]
fn unset_project_refuses_walls_cross_project_notes_and_general_review() {
    let mut s = sim_with_mork_project_unset();
    s.through_brief("fac-1", "mork");
    s.through_brief("web-1", "web");

    // `mork test --walls` is refused even on a full card of project `mork` when `[mork] project` is unset.
    let err = s
        .cmd(
            Sender::Agent(AgentKey::new("fac-1/foreman")),
            json!({"test.lease": {"kind": "walls", "command": ["t1_walls"]}}),
        )
        .unwrap_err();
    assert!(err.message.contains("--walls"), "{err:?}");

    // Cross-project `notes.add --project` to `mork` is refused without naming a Mork project.
    let err = s
        .cmd(
            Sender::Agent(AgentKey::new("web-1/foreman")),
            json!({"notes.add": {"kind": "feedback", "text": "Bug.", "project": "mork"}}),
        )
        .unwrap_err();
    assert!(err.message.contains("web"), "{err:?}");
    assert!(!err.message.contains("--project"), "refusal must not name a Mork project: {}", err.message);

    // An orchestrator (including `mork`'s) cannot write `general` memory when `[mork] project` is unset.
    let err = s
        .cmd(
            Sender::Orchestrator(ProjectId::new("mork")),
            json!({"memory.write": {"memory": "general", "name": "lesson", "description": "d", "text": "t"}}),
        )
        .unwrap_err();
    assert!(err.message.contains("general"), "{err:?}");

    // `orchestrator.review memories general` is refused and records no `reviewed_ms`.
    let err = s.operator(json!({"orchestrator.review": {"section": "memories", "target": "general"}})).unwrap_err();
    assert!(err.message.contains("general"), "{err:?}");
    assert!(s.state.memory_tallies.get("general").is_none_or(|t| t.reviewed_ms.is_none()));
}

#[test]
fn closing_schema_switches_on_mork_project() {
    let one = closing_schema(false);
    let props = one["properties"].as_object().unwrap();
    assert!(props.contains_key("feedback") && !props.contains_key("mork_feedback"));

    let two = closing_schema(true);
    let props_two = two["properties"].as_object().unwrap();
    assert!(props_two.contains_key("feedback") && props_two.contains_key("mork_feedback"));
}

#[test]
fn unset_project_closing_turn_uses_feedback_alone() {
    let mut s = Sim::new();
    s.config.mork.project = None;
    s.board("web-1", "web");
    s.operator(json!({"start": {"card": "web-1"}})).unwrap();
    s.settle_actions();
    let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == "web-1"))[0].clone();
    s.end_job(&clone, json!({}));
    s.settle_actions();

    for label in ["Scout 1", "Scout 2"] {
        let id = s.turn_started("web-1/planner");
        finish_with_tools(&mut s, &id, reply("working", label));
        s.operator(json!({"say": {"place": "web-1/spec", "text": "Continue."}})).unwrap();
    }
    let id = s.turn_started("web-1/planner");
    let mut recap = reply("brief-ready", "The plan");
    recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
    finish_with_tools(&mut s, &id, recap);
    s.turn("web-1/foreman", reply("working", "No objection"));
    s.operator(json!({"approve": {"card": "web-1"}})).unwrap();

    // Closing prompt mentions `feedback` alone and schema has only `feedback`.
    let (closing_id, prompt) = s.turn_started_with_argv("web-1/planner", &["x"]);
    assert!(prompt.contains("`feedback`") && !prompt.contains("mork_feedback"), "{prompt}");
    let saved_schema: Value = serde_json::from_str(&s.state.jobs[&closing_id].io_files["schema.json"]).unwrap();
    assert!(saved_schema["properties"].get("mork_feedback").is_none());

    // Passing `mork_feedback` is refused; passing `feedback` alone succeeds and files one project note.
    finish_with_tools(&mut s, &closing_id, json!({"feedback": "Project issue", "mork_feedback": "Mork issue"}));
    let (retry_id, retry_prompt) = s.turn_started_with_argv("web-1/planner", &["x"]);
    assert!(
        retry_prompt.contains("`mork_feedback` isn't one of the closing turn's answers")
            && retry_prompt.contains("Reply again with your answer, `feedback`"),
        "{retry_prompt}"
    );
    finish_with_tools(&mut s, &retry_id, json!({"feedback": "Project issue"}));

    let notes: Vec<_> = s.state.notes.values().collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].project.as_deref(), Some("web"));
    assert!(matches!(&notes[0].what, NoteWhat::Feedback { text } if text == "Project issue"));
}

#[test]
fn internals_hide_off_allows_provenance_but_still_refuses_home_paths() {
    let off = Internals::default();
    assert!(!off.hide);
    for sample in [
        "Co-Authored-By: Claude <noreply@anthropic.com>",
        "Generated with Mork",
        "🤖 automated",
        "Mork built this",
        "reviewer-1 found a bug",
        "Claude and foreman",
    ] {
        assert_eq!(internals(&off, "/home/u/.local/share/mork", sample), None, "hide=false should allow: {sample}");
    }
    assert_eq!(
        internals(&off, "/home/u/.local/share/mork", "see /home/u/.local/share/mork/clones/c1"),
        Some("a path inside Mork's home".to_owned())
    );

    let on = Internals { hide: true, ..Internals::default() };
    for blocked in ["Co-Authored-By: Someone", "Generated with tool", "🤖", "reviewer-1", "Claude", "path /f/x"] {
        assert!(internals(&on, "/f", blocked).is_some(), "hide=true should block: {blocked}");
    }
    assert_eq!(internals(&on, "/f", "plain commit message"), None);
}

#[test]
fn internals_hide_on_refuses_mork_in_any_case() {
    let on = Internals { hide: true, ..Internals::default() };
    for hit in ["Mork", "mork", "built with MORK"] {
        assert_eq!(internals(&on, "/f", hit), Some("`mork`".to_owned()));
    }
    assert_eq!(internals(&on, "/f", "smork"), None);
    let fixed_only = Internals { hide: true, words: Vec::new() };
    assert_eq!(internals(&fixed_only, "/f", "mork"), Some("`mork`".to_owned()));
}

#[test]
fn declined_pr_comment_names_nobody() {
    let mut s = Sim::new();
    s.through_brief("ex-1", "ex");
    s.built("ex-1", "h1", "base1");
    s.checks("ex-1", 0, &[]);
    s.candidate("ex-1");
    s.round_asked("ex-1");
    let finding = json!({"file": "a.rs", "line": 1, "summary": "[reviewer-1] Bug", "proof": "proof"});
    s.round_ends("ex-1", &[], &[finding]);
    s.proofs_copied();
    s.suite("ex-1", 0);

    // Attach a PR and posted comment to the review card so `--declined` queues a ReplyComment.
    let rc_id = CardId::new(s.review_card("ex-1"));
    let mut rc: Card = s.card(rc_id.as_str());
    rc.pr = Some(42);
    rc.head = Some(sha("h1"));
    rc.review.as_deref_mut().unwrap().raised[0].comment = Some(99);
    s.state.cards.insert(rc_id.clone(), rc);

    s.operator(json!({"resolve": {"card": rc_id, "n": 1, "declined": true}})).unwrap();
    let reply = s.pending().into_iter().find(|a| matches!(a.what, ActionWhat::ReplyComment { .. })).unwrap();
    match reply.what {
        ActionWhat::ReplyComment { body, .. } => assert_eq!(body, "This finding was declined."),
        other => panic!("expected ReplyComment, got {other:?}"),
    }
}

#[test]
fn train_run_on_unset_project_with_explicit_walled_phase_remembers_without_unwalled() {
    let mut s = sim_with_mork_project_unset();
    s.through_brief("fac-1", "mork");
    s.built("fac-1", "h1", "base1");
    s.checks("fac-1", 0, &[]);
    s.candidate("fac-1");
    s.round_asked("fac-1");
    s.round_ends("fac-1", &[], &[]);
    let job_id = s.open_jobs(|p| matches!(p, Purpose::TrainRun { test: Some(_), .. }))[0].clone();
    let k = match &s.card("fac-1").landing {
        Some(mork_types::card::Landing::Test(k)) => k.clone(),
        other => panic!("expected Landing::Test, got {other:?}"),
    };
    let head = k.commit.unwrap();
    let tree = k.tree.unwrap();
    let co = mork_types::work::Checkout {
        head: Some(head.clone()),
        tree: Some(tree.clone()),
        tracked_clean: true,
        clean: true,
    };
    let run = FinalRun {
        head,
        before: co.clone(),
        after: co,
        checks: None,
        ran: true,
        exit: Some(0),
        output: String::new(),
        phase: None,
        failing: None,
        logs: Default::default(),
    };
    s.end_job(&job_id, serde_json::to_value(run).unwrap());
    assert!(s.state.passes.iter().any(|p| p.project == "mork" && p.tree == tree && p.command == vec!["npm test"]));
}
