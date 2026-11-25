// fac-721: review_tag, tickets, and door schema_hint are driven by configuration.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use common::{Sim, sha};
use factory_core::decide::Event;
use factory_core::door::{policy, target};
use factory_core::queue::pr;
use factory_core::views::{outside::Outside, screen};
use factory_types::action::{ActionStatus, ActionWhat};
use factory_types::config::TicketsConfig;
use factory_types::door::{Op, SqlSelect};
use factory_types::door_config::{Limits, LimitsLowered, Login, Sql, TARGETS_VERSION, Target, TargetsFile, Transport};
use factory_types::work::OpenPr;
use factory_types::{CardId, Sha};
use serde_json::json;

const NEUTRAL_SCHEMA_REFUSED: &str = "refused: reading information_schema or the sys schema is not allowed because it scans metadata across every database on the server.";

fn open_pr(pr_num: u64, title: &str, dest: &str, head: &Sha) -> OpenPr {
    OpenPr {
        pr: pr_num,
        author: "colleague".into(),
        title: title.into(),
        branch: format!("feat-{pr_num}"),
        head: head.clone(),
        ours: false,
        dest: dest.into(),
        updated: String::new(),
        link: String::new(),
        description: String::new(),
        tickets: Vec::new(),
    }
}

#[test]
fn review_tag_gates_non_base_auto_review_and_head_following() {
    let mut sim = Sim::new();
    let h1 = sha("h1");
    let h2 = sha("h2");
    let h3 = sha("h3");

    // Without review_tag configured, a tagged PR targeting a non-base branch is ignored.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(10, "Fix @review-bot", "develop", &h1)] });
    assert!(!sim.state.cards.contains_key(&CardId::new("pr-ex-10")));

    // With review_tag configured, an untagged non-base PR is still ignored.
    sim.config.projects.get_mut("ex").unwrap().review_tag = Some("@review-bot".into());
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(11, "Fix without tag", "develop", &h1)] });
    assert!(!sim.state.cards.contains_key(&CardId::new("pr-ex-11")));

    // A non-base PR matching review_tag starts an auto review card on that dest.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(12, "Fix @review-bot", "develop", &h1)] });
    let card_id = CardId::new("pr-ex-12");
    let c = sim.state.cards.get(&card_id).unwrap();
    assert_eq!(c.review.as_ref().and_then(|r| r.dest.as_deref()), Some("develop"));

    // Moving the head after dropping the tag does not follow an auto-started card on a tagged project.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(12, "Fix untagged", "develop", &h2)] });
    let c = sim.state.cards.get(&card_id).unwrap();
    assert_eq!(c.head.as_ref(), Some(&h1));

    // Moving the head with the tag back follows and updates r.dest.
    sim.apply(Event::OpenPrsRead { project: "ex".into(), prs: vec![open_pr(12, "Fix @review-bot", "release", &h3)] });
    let c = sim.state.cards.get(&card_id).unwrap();
    assert_eq!(c.review.as_ref().and_then(|r| r.dest.as_deref()), Some("release"));
}

#[test]
fn tickets_config_gates_ticket_parsing_pr_text_branch_and_screen_detail() {
    let mut sim = Sim::new();
    sim.through_brief("ex-1", "ex");

    // Put an Azure DevOps ticket note at the top of the card's description.
    let ticket_desc = "andrei 21.11: Azure DevOps 4123\n\nThe description.\n";
    sim.state.board.get_mut(&CardId::new("ex-1")).unwrap().description = ticket_desc.into();
    let mut card = sim.card("ex-1");
    card.description = ticket_desc.into();
    card.pr = Some(88);
    sim.state.cards.insert(CardId::new("ex-1"), card.clone());

    // Without [projects.ex.tickets], tickets and DevOps/PR screen detail are inactive.
    assert!(!pr::pr_title(&sim.state, &card, &sim.config).contains("#4123"));
    let view = screen::screen(&sim.state, &Outside::default(), sim.ctx());
    let row = view.rooms.iter().find(|r| r.place.as_str() == "ex-1").unwrap();
    assert_eq!(row.under.as_ref().unwrap().detail, "");

    sim.board("ex-2", "ex");
    sim.state.board.get_mut(&CardId::new("ex-2")).unwrap().description = ticket_desc.into();
    sim.andrei(json!({"start": {"card": "ex-2", "mode": "full", "no-planner": true}})).unwrap();
    sim.settle_actions();
    assert_eq!(sim.card("ex-2").branch.as_deref(), Some("ex-2"));

    // Enable [projects.ex.tickets]: screen detail, PR title, branch naming, and PR wording hold activate.
    sim.config.projects.get_mut("ex").unwrap().tickets = Some(TicketsConfig {
        api: "https://example.invalid/api/workitems".into(),
        page: "https://example.invalid/workitems/edit/".into(),
    });
    let view = screen::screen(&sim.state, &Outside::default(), sim.ctx());
    let row = view.rooms.iter().find(|r| r.place.as_str() == "ex-1").unwrap();
    assert_eq!(row.under.as_ref().unwrap().detail, "PR 88 DevOps 4123");
    assert!(pr::pr_title(&sim.state, &card, &sim.config).starts_with("#4123 "));

    sim.board("ex-3", "ex");
    sim.state.board.get_mut(&CardId::new("ex-3")).unwrap().description = ticket_desc.into();
    sim.andrei(json!({"start": {"card": "ex-3", "mode": "full", "no-planner": true}})).unwrap();
    sim.settle_actions();
    assert!(sim.card("ex-3").branch.as_deref().is_some_and(|b| b.starts_with("4123-")));
}

#[test]
fn tickets_config_holds_unworded_open_pr_and_formats_configured_page_url() {
    let mut sim = Sim::new();
    sim.config.projects.get_mut("ex").unwrap().tickets = Some(TicketsConfig {
        api: "https://example.invalid/api/workitems".into(),
        page: "https://example.invalid/workitems/edit/".into(),
    });
    sim.through_brief("ex-1", "ex");
    sim.state.board.get_mut(&CardId::new("ex-1")).unwrap().description =
        "andrei 21.11: Azure DevOps 4123\n\nThe description.\n".into();
    sim.built("ex-1", "h1", "base1");
    sim.checks("ex-1", 0, &[]);
    sim.candidate("ex-1");
    sim.round_asked("ex-1");
    sim.round_ends("ex-1", &[], &[]);
    sim.suite("ex-1", 0);
    let held = sim.state.actions.values().find(|a| matches!(a.what, ActionWhat::OpenPr { .. })).unwrap().clone();
    assert_eq!(held.status, ActionStatus::Held);
    assert!(held.words.as_deref().is_some_and(|e| e.contains("PR wording")), "{:?}", held.words);
    assert!(pr::held_words("refused").contains("on a project with tickets configured"));

    // Adding PR wording and retrying OpenPr formats the title and configured page URL.
    sim.state.board.get_mut(&CardId::new("ex-1")).unwrap().description =
        "foreman 21.11: PR wording\n  Title: Fix billing totals\n  4123: Recalculate invoice totals on save.\n\
         andrei 21.11: Azure DevOps 4123\n\nThe description.\n"
            .into();
    sim.andrei(json!({"action.retry": {"action": held.id}})).unwrap();
    let retried = sim.pending().into_iter().find(|a| matches!(a.what, ActionWhat::OpenPr { .. })).unwrap();
    match retried.what {
        ActionWhat::OpenPr { title, description, .. } => {
            assert_eq!(title, "#4123 Fix billing totals");
            assert_eq!(
                description,
                "[4123](https://example.invalid/workitems/edit/4123): Recalculate invoice totals on save."
            );
        }
        other => panic!("expected OpenPr, got {other:?}"),
    }
}

fn sql_target(database: &str, schema_hint: Option<&str>) -> Target {
    Target {
        description: "Test DB".into(),
        transport: Transport::Local,
        ssh: None,
        limits: LimitsLowered::default(),
        sql: Some(Sql {
            login: Login { section: "db".into(), user: "user".into(), password: "password".into() },
            host: "127.0.0.1".into(),
            port: 3306,
            database: database.into(),
            read_only: true,
            timeout_ms: None,
            schema_hint: schema_hint.map(str::to_owned),
        }),
        mongo: None,
        libra: None,
        file: None,
        sh: BTreeMap::new(),
        misc: false,
    }
}

#[test]
fn schema_refusal_is_neutral_and_uses_target_schema_hint_and_door_help() {
    let hint = "Read migrations/ in the repo instead.";
    let mut targets = BTreeMap::new();
    targets.insert("plain".into(), sql_target("app", None));
    targets.insert("hinted".into(), sql_target("app", Some(hint)));
    targets.insert("hinted_sys".into(), sql_target("information_schema", Some(hint)));
    let file = TargetsFile {
        version: TARGETS_VERSION,
        access: Some("/etc/access.md".into()),
        defaults: Limits::default(),
        targets,
    };

    // Plain target: neutral refusal with no hint; door.help omits schema_hint.
    let err = policy::resolve(
        &file,
        "plain",
        &Op::SqlSelect(SqlSelect { query: "SELECT * FROM information_schema.tables".into() }),
    )
    .unwrap_err();
    assert_eq!(err, NEUTRAL_SCHEMA_REFUSED);
    let plain_help = target::describe(&file, "plain", &file.targets["plain"]);
    assert!(plain_help["sql"].get("schema_hint").is_none());

    // Hinted target: SQL parser refusal site appends schema_hint; door.help exposes schema_hint.
    let err = policy::resolve(&file, "hinted", &Op::SqlSelect(SqlSelect { query: "SELECT * FROM sys.version".into() }))
        .unwrap_err();
    assert_eq!(err, format!("{NEUTRAL_SCHEMA_REFUSED} {hint}"));
    let hinted_help = target::describe(&file, "hinted", &file.targets["hinted"]);
    assert_eq!(hinted_help["sql"]["schema_hint"], hint);

    // Hinted target whose configured database is information_schema: policy database refusal site appends schema_hint.
    let err = policy::resolve(&file, "hinted_sys", &Op::SqlSelect(SqlSelect { query: "SELECT 1".into() })).unwrap_err();
    assert_eq!(err, format!("{NEUTRAL_SCHEMA_REFUSED} {hint}"));
}
