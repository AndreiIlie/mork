// fac-653: cards walked from start to done through `decide` alone, in memory: the two ways a card lands, and each
// way back to its foreman before it lands.

mod common;

use common::{Sim, reply, sha};
use mork_core::decide::{Decision, Event, Intent};
use mork_core::queue::order::in_queue;
use mork_core::state::Change;
use mork_types::action::ActionWhat;
use mork_types::card::{Landing, Stage};
use mork_types::lookback::Read;
use mork_types::protocol::ErrorCode;
use mork_types::state::{MessageKind, Sender};
use mork_types::work::Purpose;
use mork_types::{AgentKey, Command, catalog, words};
use serde_json::{Value, json};

const LOCAL: &str = "web-1";
const PR: &str = "ex-1";

#[test]
fn only_an_orchestrator_can_wait_for_wakes() {
    let mut sim = Sim::new();
    for sender in [Sender::Operator, Sender::Agent("web-1/foreman".into())] {
        let result = sim.cmd(sender, json!({"wakes": {}}));
        assert!(matches!(result, Err(e) if e.code == ErrorCode::Buzz && e.message.contains("`wakes`")));
    }
    let result = sim.cmd(Sender::Orchestrator("web".into()), json!({"wakes": {}}));
    assert!(matches!(result, Err(e) if e.code == ErrorCode::Refused && e.message == "`wakes` isn't decided here"));
}

fn finding() -> Value {
    json!({"file": "src/a.rs", "line": 3, "summary": "src/a.rs is wrong", "proof": "a proof"})
}

// The card's foreman is owed a turn for the engine's latest request to it in the card's room, which says `words`.
fn foreman_asked(s: &Sim, card: &str, words: &str) {
    let requests: Vec<_> =
        s.engine_to(card, "foreman").into_iter().filter(|m| m.kind == MessageKind::Request).collect();
    let last = requests.last().map(|m| m.text.as_str()).unwrap_or_default();
    assert!(last.contains(words), "the foreman's last request isn't about {words:?}: {last}");
    assert!(s.owed(&format!("{card}/foreman")), "{card}'s foreman isn't woken");
}

// The submitted head passes its checks and, containing the base, is verification's candidate itself: no checks run
// again, and its round and its suite are owed side by side.
fn verifying(s: &mut Sim, card: &str) {
    assert_eq!(s.stage(card), Stage::Checks);
    s.checks(card, 0, &[]);
    assert_eq!(s.stage(card), Stage::Merging);
    assert!(matches!(s.card(card).landing, Some(Landing::Prepare(_))));
    s.candidate(card);
    assert_eq!(s.stage(card), Stage::Merging);
    assert!(matches!(s.card(card).landing, Some(Landing::Test(k)) if k.checked && k.commit == s.card(card).head));
    let again = s.open_jobs(|p| matches!(p, Purpose::TrainRun { checks: true, .. } | Purpose::Check { .. }));
    assert!(again.is_empty(), "the checked head's checks run again: {again:?}");
    assert!(!s.open_jobs(|p| matches!(p, Purpose::TrainRun { checks: false, .. })).is_empty(), "no suite owed");
}

// The local card's suite passes, which alone queues nothing; its round passes, verifying the fixes of the findings
// numbered `fixed`, and it lands by moving the base.
fn lands_locally(s: &mut Sim, card: &str, fixed: &[u64]) {
    s.suite(card, 0);
    assert_eq!(s.stage(card), Stage::Merging);
    assert!(matches!(s.card(card).landing, Some(Landing::Review(_))));
    assert!(!in_queue(&s.card(card)) && !s.landing_asked(card), "the suite alone queued {card}");
    s.round_asked(card);
    pass_and_land(s, card, fixed);
}

fn pass_and_land(s: &mut Sim, card: &str, fixed: &[u64]) {
    s.round_ends(card, fixed, &[]);
    assert_eq!(s.stage(card), Stage::Merging);
    assert!(in_queue(&s.card(card)), "the round's pass didn't queue {card}");
    let moved = s.action_of(|w| matches!(w, ActionWhat::MoveBase { card: c, .. } if c.as_str() == card));
    s.returned(&moved.id, Ok(json!({})));
    s.settle_actions();
    assert_eq!(s.stage(card), Stage::Done);
}

// The card's foreman fixes what came back with head `head`, forked from `fork`, and the fix lands, its round
// verifying the fixes of the findings numbered `fixed`.
fn fix_lands(s: &mut Sim, card: &str, head: &str, fork: &str, fixed: &[u64]) {
    s.built(card, head, fork);
    verifying(s, card);
    lands_locally(s, card, fixed);
}

// The merge action and its run asked in one decision.
fn merge_asked(d: &Decision) -> bool {
    let merge = d.changes.iter().find_map(|c| match c {
        Change::PutAction(a) if matches!(a.what, ActionWhat::Merge { .. }) => Some(a.id.clone()),
        _ => None,
    });
    merge.is_some_and(|id| d.intents.contains(&Intent::RunAction { id }))
}

#[test]
fn a_local_card_lands_through_the_merge_queue() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    verifying(&mut s, LOCAL);
    lands_locally(&mut s, LOCAL, &[]);
    assert_eq!(s.state.landings.get("web"), Some(&1));
}

fn assert_pr_landing_status(s: &Sim, expected: &str) {
    let view = mork_core::views::place::place(&s.state, PR, None, &Default::default(), s.ctx());
    let land = view.card.as_ref().and_then(|c| c.pipeline.last());
    assert_eq!(land.and_then(|step| step.sub.as_deref()), Some(expected));
    let summary = view.overview.as_ref().and_then(|o| o.stages.last());
    assert_eq!(summary.map(|stage| stage.summary.as_str()), Some(expected));
}

#[test]
fn a_published_pr_merges_on_its_approval() {
    let mut s = Sim::new();
    s.through_brief(PR, "ex");
    s.built(PR, "h1", "base1");
    verifying(&mut s, PR);
    s.round_asked(PR);
    s.round_ends(PR, &[], &[]);
    assert_eq!(s.stage(PR), Stage::Merging);
    assert!(matches!(s.card(PR).landing, Some(Landing::Test(_))));
    assert!(!s.landing_asked(PR) && s.card(PR).pr.is_none(), "the round alone published {PR}");
    s.suite(PR, 0);
    assert_eq!(s.stage(PR), Stage::Merging);
    assert!(matches!(s.card(PR).landing, Some(Landing::Approval { verified: true, .. })));
    assert_eq!(s.card(PR).pr, Some(7));
    assert_pr_landing_status(&s, "PR #7 waits for your approval");
    let comment = s.action_of(|w| matches!(w, ActionWhat::Comment { .. }));
    s.returned(&comment.id, Ok(json!({})));
    let unapproved = s.pr_read(PR, false);
    assert!(!merge_asked(&unapproved));
    assert!(!s.has_action(|w| matches!(w, ActionWhat::Merge { .. })), "{PR} merges unapproved");
    assert_eq!(s.stage(PR), Stage::Merging);
    let approved = s.pr_read(PR, true);
    assert!(merge_asked(&approved), "the approval asked no merge");
    assert_eq!(s.stage(PR), Stage::Merging);
    let merge = s.action_of(|w| matches!(w, ActionWhat::Merge { .. }));
    s.returned(&merge.id, Ok(json!({"commit": common::sha("merged")})));
    assert_eq!(s.stage(PR), Stage::Done);
}

#[test]
fn manual_pr_lands_after_the_operator_merges_it() {
    let mut s = Sim::new();
    s.config.projects.get_mut("ex").unwrap().forge = serde_json::from_value(json!({
        "kind":"github","owner":"example","repo":"repo","token":"/token"
    }))
    .unwrap();
    s.through_brief(PR, "ex");
    s.built(PR, "h1", "base1");
    verifying(&mut s, PR);
    s.round_asked(PR);
    s.round_ends(PR, &[], &[]);
    s.suite(PR, 0);
    assert!(!merge_asked(&s.pr_read(PR, true)));
    assert!(s.state.asks.values().any(|a| a.open && a.text.contains("review and merge my PR on GitHub")));
    assert_pr_landing_status(&s, "PR #7 waits for your merge");
    let c = s.card(PR);
    let seen = mork_types::work::PrSeen {
        pr: 7,
        state: mork_types::work::PrState::Merged,
        approved: false,
        head: c.pushed.unwrap(),
        dest: Some("master".into()),
        merge_commit: Some(sha("merged")),
        merge_parents: vec![sha("base1"), sha("h1")],
    };
    s.apply(Event::PrRead { card: c.id, seen, before_merge: false });
    assert_eq!(s.stage(PR), Stage::Done);
}

#[test]
fn failed_checks_go_back_to_the_foreman() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    assert_eq!(s.stage(LOCAL), Stage::Checks);
    s.checks(LOCAL, 1, &[finding()]);
    assert_eq!(s.stage(LOCAL), Stage::Build);
    foreman_asked(&s, LOCAL, "Fix the checks' findings");
    fix_lands(&mut s, LOCAL, "h2", "base1", &[]);
}

#[test]
fn a_failed_round_goes_back_to_the_foreman() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    verifying(&mut s, LOCAL);
    s.round_asked(LOCAL);
    let review = s.review_card(LOCAL);
    foreman_asked(&s, &review, "Use the `review-cards` skill to choose Quick, Standard or Deep review");
    let unlisted = json!({"file": "src/caller.rs", "line": 3, "summary": "The caller breaks", "proof": "a proof"});
    s.round_ends(LOCAL, &[], &[finding(), unlisted]);
    assert_eq!(s.stage(LOCAL), Stage::Merging);
    s.proofs_copied();
    assert_eq!(s.stage(LOCAL), Stage::Merging, "the round's failure waits for the suite");
    s.suite(LOCAL, 0);
    assert_eq!(s.stage(LOCAL), Stage::Build);
    assert!(!s.owed(&format!("{LOCAL}/foreman")), "failed round with findings must wait for triage");
    assert!(s.card(LOCAL).triage.is_some());
    s.operator(json!({"triage": {"card": LOCAL, "fix": ["1", "2"]}})).unwrap();
    assert!(!s.owed(&format!("{LOCAL}/foreman")), "the fix request didn't wait for the base's import");
    s.imported(LOCAL);
    foreman_asked(&s, LOCAL, "src/a.rs is wrong");
    foreman_asked(&s, LOCAL, "The caller breaks");
    s.built(LOCAL, "h2", "base1");
    verifying(&mut s, LOCAL);
    s.suite(LOCAL, 0);
    s.round_asked(LOCAL);
    foreman_asked(&s, &review, &format!("git diff refs/rounds/1 {}", common::sha("h2")));
    foreman_asked(&s, &review, &format!("- Round 1: `{}`", common::sha("base1")));
    foreman_asked(&s, &review, "Findings on unlisted files count like any other");
    pass_and_land(&mut s, LOCAL, &[1, 2]);
}

#[test]
fn a_failed_suite_goes_back_to_the_foreman() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    verifying(&mut s, LOCAL);
    s.suite(LOCAL, 1);
    assert_eq!(s.stage(LOCAL), Stage::Merging, "the suite's failure waits for the round");
    s.round_asked(LOCAL);
    s.round_ends(LOCAL, &[], &[]);
    assert_eq!(s.stage(LOCAL), Stage::Build);
    assert!(!s.owed(&format!("{LOCAL}/foreman")), "the fix request didn't wait for the base's import");
    s.imported(LOCAL);
    foreman_asked(&s, LOCAL, "The suite failed (exit 1)");
    fix_lands(&mut s, LOCAL, "h2", "base1", &[]);
}

#[test]
fn a_moved_base_is_joined_before_the_checks() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.tip = "base2";
    s.built(LOCAL, "h1", "base1");
    assert_eq!(s.stage(LOCAL), Stage::Build, "a head missing the base went to its checks");
    s.imported(LOCAL);
    assert_eq!(s.stage(LOCAL), Stage::Build);
    foreman_asked(&s, LOCAL, "The base moved past this head's fork");
    fix_lands(&mut s, LOCAL, "h2", "base2", &[]);
}

#[test]
fn builder_spawn_and_launch_are_gone() {
    use mork_types::Command;

    let spawn = json!({"spawn": {"card": LOCAL, "role": "builder", "task": "Implement"}});
    let launch =
        json!({"launch": {"card": LOCAL, "deliverable": 1, "task": "Implement", "model": "sol", "effort": "high"}});
    assert!(Command::parse(&spawn, false).unwrap_err().message.contains("builder"));
    assert!(Command::parse(&launch, false).unwrap_err().message.contains("unknown command"));
}

#[test]
fn a_reopened_mock_holds_the_foreman() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    let foreman = format!("{LOCAL}/foreman");
    assert!(s.owed(&foreman));
    use mork_types::card::{Mock, MockStep};

    let card = s.state.cards.get_mut(LOCAL).unwrap();
    card.visual = true;
    card.mock = Some(Mock { step: MockStep::Approved, ..Mock::default() });
    s.operator(json!({"mock": {"card": LOCAL}})).unwrap();
    s.operator(json!({"say": {"place": LOCAL, "text": "Please carry on."}})).unwrap();
    assert!(!s.owed(&foreman));
    assert!(s.owed(&format!("{LOCAL}/planner")));
}

#[test]
fn rebrief_drops_old_build_retries_and_wakes_the_foreman_for_its_specification() {
    use common::reply;
    use mork_core::decide::{Event, JobEnd, SessionSeen};
    use mork_types::state::Owed;

    let foreman = format!("{LOCAL}/foreman");
    for cut_before_brief in [true, false] {
        let mut s = Sim::new();
        s.board(LOCAL, "web");
        s.operator(json!({"start": {"card": LOCAL, "no-planner": true}})).unwrap();
        s.settle_actions();
        let clone = s.open_jobs(|p| matches!(p, Purpose::Clone { .. }))[0].clone();
        s.end_job(&clone, json!({}));
        s.settle_actions();
        let mut recap = reply("brief-ready", "The plan");
        recap["hand"]["message"] = json!("- where: src\n---\nThe plan.");
        s.turn(&foreman, recap);
        s.operator(json!({"approve": {"card": LOCAL}})).unwrap();

        let id = s.turn_started(&foreman);
        let cut = Event::JobEnded { id, end: JobEnd::Cut { session: SessionSeen::default() } };
        if cut_before_brief {
            s.apply(cut.clone());
            assert!(matches!(s.state.agents[foreman.as_str()].owed, Some(Owed::Retry { .. })));
        }
        s.operator(json!({"brief": {"card": LOCAL}})).unwrap();
        if !cut_before_brief {
            s.apply(cut);
        }
        assert!(matches!(s.state.agents[foreman.as_str()].owed, Some(Owed::Wake)));
        let next = s.turn_started(&foreman);
        assert!(s.state.jobs[&next].stdin.contains("is back at brief"));
    }
}

// why: a missing card in the start response is a test failure.
#[allow(clippy::unwrap_used)]
fn begin_lookback(s: &mut Sim, project: &str, read: Option<&str>) -> String {
    let mut cmd = json!({"lookback": {"project": project}});
    if let Some(read) = read {
        cmd["lookback"]["read"] = json!(read);
    }
    s.operator(cmd).unwrap()["lookback"]["card"].as_str().unwrap().to_owned()
}

fn clone_lookback(s: &mut Sim, card: &str) {
    s.settle_actions();
    s.answer_job(
        "look-back clone",
        |p| matches!(p, Purpose::Clone { card: c, .. } if c.as_str() == card),
        json!({"lookback": {}}),
    );
    assert_eq!(s.stage(card), Stage::Report);
}

#[test]
fn lookback_records_each_read_on_done_after_report_ready() {
    let mut s = Sim::new();
    let week = 7 * 86_400_000;
    for (name, read) in [(None, Read::Pulse), (Some("deep-sweep"), Read::DeepSweep)] {
        let card = begin_lookback(&mut s, "web", name);
        let started = s.card(&card).started_ms;
        let run = s.card(&card).lookback.unwrap();
        assert_eq!(run.read, read);
        assert_eq!(run.ended_since_ms, (read == Read::Pulse).then_some(started - week));
        clone_lookback(&mut s, &card);
        s.turn(&format!("{card}/foreman"), reply("report-ready", "Ready"));
        assert!(s.state.lookbacks.get("web").is_none_or(|reads| !reads.contains_key(&read)));
        s.operator(json!({"done": {"card": card}})).unwrap();
        let cursor = &s.state.lookbacks["web"][&read];
        assert_eq!(cursor.commit, Some(sha(s.tip)));
        assert_eq!(cursor.started_ms, Some(started));
    }
    let next = begin_lookback(&mut s, "web", Some("pulse"));
    let run = s.card(&next).lookback.unwrap();
    assert_eq!(run.from, Some(sha(s.tip)));
    assert_eq!(run.ended_since_ms, s.state.lookbacks["web"][&Read::Pulse].started_ms);
    assert_eq!(run.since_ms, None);
}

#[test]
fn lookback_refuses_a_retired_read_and_a_second_live_read() {
    let mut s = Sim::new();
    let error = s.operator(json!({"lookback": {"project": "web", "read": "merged"}})).unwrap_err();
    assert!(error.message.contains("unknown look-back read"));
    let card = begin_lookback(&mut s, "web", Some("pulse"));
    let error = s.operator(json!({"lookback": {"project": "web", "read": "deep-sweep"}})).unwrap_err();
    assert!(error.message.contains(&card));
}

#[test]
fn lookback_done_before_report_ready_leaves_the_cursor_unchanged() {
    let mut s = Sim::new();
    let first = begin_lookback(&mut s, "web", None);
    clone_lookback(&mut s, &first);
    s.turn(&format!("{first}/foreman"), reply("report-ready", "Ready"));
    s.operator(json!({"done": {"card": first}})).unwrap();
    let previous = s.state.lookbacks["web"][&Read::Pulse].clone();
    s.tip = "base2";
    for cloned in [false, true] {
        let card = begin_lookback(&mut s, "web", None);
        if cloned {
            clone_lookback(&mut s, &card);
            assert_eq!(s.card(&card).lookback.unwrap().through, Some(sha(s.tip)));
        }
        s.operator(json!({"done": {"card": card}})).unwrap();
        assert_eq!(s.stage(&card), Stage::Done);
        assert_eq!(s.state.lookbacks["web"][&Read::Pulse], previous);
    }
}

#[test]
fn lookback_fetches_a_bitbucket_project_with_a_cursor_while_idle() {
    let mut s = Sim::new();
    for project in ["web", "ex"] {
        let card = begin_lookback(&mut s, project, Some("deep-sweep"));
        clone_lookback(&mut s, &card);
        s.turn(&format!("{card}/foreman"), reply("report-ready", "Ready"));
        s.operator(json!({"done": {"card": card}})).unwrap();
    }
    s.settle_actions();
    s.now += 5 * 60 * 1000;
    s.apply(Event::Tick { claude_account: None });
    let projects: Vec<_> = s
        .pending()
        .into_iter()
        .filter_map(|action| match action.what {
            ActionWhat::Fetch { project, .. } => Some(project),
            _ => None,
        })
        .collect();
    assert_eq!(projects, ["ex"]);
}

#[test]
fn proposal_notes_keep_ordered_literal_paths_under_the_seats_project() {
    let mut s = Sim::new();
    let card = begin_lookback(&mut s, "web", None);
    clone_lookback(&mut s, &card);
    let spec = catalog::find("notes.add", false).unwrap();
    let args =
        ["--file", "src/a,b.rs", "proposal", "Share the parser.", "--title", "Use one parser", "--file", "[src]"];
    let cmd = words::command(spec, &args.map(str::to_owned)).unwrap();
    assert!(Command::parse(&cmd, false).is_ok());
    let sender = Sender::Agent(AgentKey::new(format!("{card}/foreman")));
    let note = s.cmd(sender.clone(), cmd.clone()).unwrap();
    assert_eq!(note["notes.add"]["kind"], "proposal");
    assert_eq!(note["notes.add"]["project"], "web");
    assert_eq!(note["notes.add"]["card"], card);
    assert_eq!(note["notes.add"]["title"], "Use one parser");
    assert_eq!(note["notes.add"]["text"], "Share the parser.");
    assert_eq!(note["notes.add"]["files"], json!(["src/a,b.rs", "[src]"]));
    s.config.mork.project = Some("ex".into());
    let mut wrong_project = cmd;
    wrong_project["notes.add"]["project"] = json!("ex");
    let refusal = s.cmd(sender, wrong_project).unwrap_err();
    assert!(refusal.message.contains("this seat files notes under its own project"));
    let without_files = s
        .operator(json!({"notes.add": {
            "kind": "proposal", "project": "web", "title": "Group look-back notes", "text": "Group related work."
        }}))
        .unwrap();
    assert_eq!(without_files["notes.add"]["files"], json!([]));
}

#[test]
fn proposal_notes_refuse_wrong_fields_and_keep_the_smell_syntax() {
    let mut s = Sim::new();
    let invalid = [
        json!({"text": "Work", "kind": "proposal"}),
        json!({"text": "Work", "kind": "proposal", "title": " "}),
        json!({"text": "Work", "kind": "proposal", "title": "Title\nSubtitle"}),
        json!({"text": " ", "kind": "proposal", "title": "Title"}),
        json!({"text": "Work", "kind": "proposal", "title": "Title", "line": 1}),
        json!({"text": "Work", "kind": "proposal", "title": "Title", "why": "Reason"}),
        json!({"text": "Work", "kind": "proposal", "title": "Title", "file": "src/a.rs"}),
        json!({"text": "Work", "kind": "feedback", "title": "Title"}),
        json!({"text": "Work", "kind": "lesson", "title": "Title"}),
        json!({"text": "Work", "kind": "smell", "title": "Title", "file": "src/a.rs", "line": 1, "why": "Reason"}),
    ];
    for mut fields in invalid {
        fields["project"] = json!("web");
        assert!(s.operator(json!({"notes.add": fields})).is_err(), "{fields}");
    }
    let spec = catalog::find("notes.add", false).unwrap();
    let smell = ["smell", "Work", "--project", "web", "--file", "src/a,b.rs", "--line", "1", "--why", "Reason"];
    let cmd = words::command(spec, &smell.map(str::to_owned)).unwrap();
    assert_eq!(cmd["notes.add"]["file"], "src/a,b.rs");
    assert!(Command::parse(&cmd, false).is_ok());
    let note = s.operator(cmd).unwrap();
    assert_eq!(note["notes.add"]["file"], "src/a,b.rs");
    for kind in ["smell", "feedback", "lesson"] {
        let args = [kind, "Work", "--file", "src/a.rs", "--file", "src/b.rs"];
        assert!(words::command(spec, &args.map(str::to_owned)).is_err(), "{kind}");
    }
}

#[test]
fn smell_notes_keep_positional_file_line_and_why() {
    let mut s = Sim::new();
    let spec = catalog::find("notes.add", false).unwrap();
    let forms: &[&[&str]] = &[
        &["smell", "--text", "Duplicate parser", "--project", "web", "--file", "src/parser.rs", "42", "Can drift"],
        &["smell", "src/parser.rs", "42", "Can drift", "web", "--text", "Duplicate parser"],
        &[
            "smell",
            "--text",
            "Duplicate parser",
            "--project",
            "web",
            "--file",
            "src/parser.rs",
            "--line",
            "42",
            "Can",
            "drift",
        ],
    ];
    for form in forms {
        let args: Vec<_> = form.iter().map(|word| (*word).to_owned()).collect();
        let cmd = words::command(spec, &args).unwrap();
        let answer = s.operator(cmd).unwrap();
        let note = &answer["notes.add"];
        assert_eq!(note["file"], "src/parser.rs");
        assert_eq!(note["line"], 42);
        assert_eq!(note["why"], "Can drift");
    }
}

#[test]
fn a_passing_checks_report_sends_test_changes_to_the_round() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    let flag = "tests/example.rs:3 (base): deleted test in example";
    let head = sha("h1");
    let run = json!({"head": head, "exit": 0, "folder_dirty": false, "folder_commit": "c1", "located": [],
        "report": {"status": 0, "findings": [], "test_changes": [flag], "head": head, "base": sha("base1")}});
    s.answer_job("check", |p| matches!(p, Purpose::Check { .. }), run);
    s.settle_actions();
    s.candidate(LOCAL);
    s.round_asked(LOCAL);
    foreman_asked(&s, &s.review_card(LOCAL), flag);
}

#[test]
fn a_secret_in_the_submitted_history_stops_verification() {
    let mut s = Sim::new();
    s.through_brief(LOCAL, "web");
    s.built(LOCAL, "h1", "base1");
    let hit = "a credential, in fixture:2 (commit h1)";
    s.answer_job("check", |p| matches!(p, Purpose::Check { .. }), json!({"secret_hit": hit}));
    assert_eq!(s.stage(LOCAL), Stage::Build);
    assert!(s.card(LOCAL).gates.checking.is_none());
    assert!(s.open_jobs(|p| matches!(p, Purpose::Candidate { .. })).is_empty());
    foreman_asked(&s, LOCAL, hit);
}
