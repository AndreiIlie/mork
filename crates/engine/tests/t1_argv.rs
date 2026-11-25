// fac-25: an unwalled seat's settings deny it the access file and ssh.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use factory_engine::cli::{ContextReport, StreamState, agy, claude, codex};
use factory_engine::jobs::context::Context;
use factory_engine::paths::Home;
use factory_types::config::Config;
use factory_types::{AgentKey, JobId};
use serde_json::json;

fn config(extra: &str) -> Config {
    factory_engine::config::parse(&format!("[factory]\nproject = \"f\"\n[git]\nauthor = \"A <a@b>\"\n{extra}")).unwrap()
}

// The seats run on Claude here, whichever CLI the roster gives them (fac-399).
const CLAUDE: &str = "cli = \"claude\"\nmodel = \"claude-opus-5-5\"\n";

// fac-25: an unwalled seat's settings carry its deny list, every turn of it; a walled seat's don't.
#[test]
fn fac_25_an_unwalled_seats_settings_deny_access_md_and_ssh() {
    let schema = json!({"type": "object"});
    let deny = json!(["Read(//**/ACCESS.md)", "Read(//**/.ssh/**)", "Bash(*ACCESS.md*)", "Bash(*.ssh*)"]);
    let researcher = config(&format!("[seats.researcher]\n{CLAUDE}wall = \"none\"\n")).seat("researcher").unwrap();
    assert_eq!(researcher.wall, factory_types::config::Wall::None);
    for resume in [false, true] {
        let argv = claude::argv(&researcher, "uuid-1", resume, &schema, claude::Dirs::default(), None, Some(200_000));
        assert_eq!(argv[argv.iter().position(|a| a == "--permission-mode").unwrap() + 1], "bypassPermissions");
        let i = argv.iter().position(|a| a == "--settings").unwrap();
        let settings: serde_json::Value = serde_json::from_str(&argv[i + 1]).unwrap();
        assert_eq!(settings["permissions"]["deny"], deny);
        assert_eq!(settings["attribution"]["sessionUrl"], false);
    }
    let walled = config(&format!("[seats.researcher]\n{CLAUDE}")).seat("researcher").unwrap();
    let argv = claude::argv(&walled, "uuid-1", false, &schema, claude::Dirs::default(), None, Some(200_000));
    let mut settings: serde_json::Value = serde_json::from_str(argv.last().unwrap()).unwrap();
    assert!(settings.as_object_mut().unwrap().remove("hooks").is_some());
    assert_eq!(settings, serde_json::from_str::<serde_json::Value>(claude::SETTINGS).unwrap());
    let foreman = format!("[seats.foreman]\n{CLAUDE}wall = \"none\"\nadvisor = \"claude-fable-5-1\"\n");
    let unwalled = config(&foreman).seat("foreman").unwrap();
    let argv = claude::argv(&unwalled, "uuid-1", false, &schema, claude::Dirs::default(), None, Some(200_000));
    let settings: serde_json::Value = serde_json::from_str(argv.last().unwrap()).unwrap();
    assert_eq!(
        (settings["permissions"]["deny"].clone(), settings["advisorModel"].clone()),
        (deny, "claude-fable-5-1".into())
    );
}

#[test]
fn fac_681_context_cap_passes_autocompact_to_claude_and_env_to_agy() {
    let schema = json!({"type": "object"});
    let claude_seat = config(&format!("[seats.foreman]\n{CLAUDE}")).seat("foreman").unwrap();
    let capped = claude::argv(&claude_seat, "uuid-1", false, &schema, claude::Dirs::default(), None, Some(200_000));
    let i = capped.iter().position(|a| a == "--autocompact").expect("capped claude argv carries --autocompact");
    assert_eq!(capped[i + 1], "200000");

    let uncapped = claude::argv(&claude_seat, "uuid-1", false, &schema, claude::Dirs::default(), None, None);
    assert!(!uncapped.iter().any(|a| a == "--autocompact"), "disabled cap omits --autocompact: {uncapped:?}");

    let job = JobId::new("job-1");
    let owner = AgentKey::new("fac-681/foreman");
    let agy_cfg = config("[seats.foreman]\ncli = \"agy\"\nmodel = \"gemini-4-neon\"\n");
    let agy_seat = agy_cfg.seat("foreman").unwrap();
    let ctx = Context::new(Home::new(PathBuf::from("/tmp/f")), agy_cfg, PathBuf::from("/bin/f"), BTreeMap::new());
    let env = ctx.env(&job, &owner, Some(&agy_seat), &[], Path::new("/tmp/f/io"));
    assert_eq!(env.get("AGY_CLI_AUTO_COMPACT_OVERRIDE").map(String::as_str), Some("200000"));

    let disabled_cfg = factory_engine::config::parse(
        "[factory]\nproject = \"f\"\ncontext_cap = 0\n[git]\nauthor = \"A <a@b>\"\n[seats.foreman]\ncli = \"agy\"\nmodel = \"gemini-4-neon\"\n",
    )
    .unwrap();
    let ctx = Context::new(Home::new(PathBuf::from("/tmp/f")), disabled_cfg, PathBuf::from("/bin/f"), BTreeMap::new());
    let env = ctx.env(&job, &owner, Some(&agy_seat), &[], Path::new("/tmp/f/io"));
    assert!(!env.contains_key("AGY_CLI_AUTO_COMPACT_OVERRIDE"));
}

fn agy_step(st: &mut StreamState, conv: &str, idx: u64, kind: &str, usage: Option<(u64, u64, u64)>) {
    let mut step = json!({"conversation_id": conv, "step_index": idx, "state": "DONE", "step_type": kind});
    if let Some((input, cache, output)) = usage {
        step["usage"] = json!({"input_tokens": input, "cache_read_tokens": cache, "output_tokens": output});
    }
    agy::read_line(st, &json!({"event": "step_update", "step_update": step}).to_string());
}

#[test]
fn fac_681_agy_stream_tracks_main_context_compaction_and_ignores_subagents() {
    let mut st = StreamState::default();
    let init = json!({"event": "init", "conversation_id": "conv-main", "init": {"model": "gemini-4-neon-high"}});
    agy::read_line(&mut st, &init.to_string());

    // Pre-compaction call updates answered and live context.
    agy_step(&mut st, "conv-main", 1, "agent_response", Some((10_000, 170_000, 500)));
    assert!(st.answered && !st.compacted);
    assert_eq!(
        st.context,
        Some(ContextReport { used: 180_500, window: None, model: Some("gemini-4-neon-high".into()) })
    );

    // Subagent events never overwrite parent context or mark parent compaction.
    agy_step(&mut st, "conv-sub", 0, "checkpoint", Some((500, 0, 50)));
    agy_step(&mut st, "conv-sub", 1, "agent_response", Some((1_200, 0, 100)));
    assert!(!st.compacted);
    assert_eq!(st.context.as_ref().map(|c| c.used), Some(180_500));

    // Main checkpoint marks compaction without replacing context with the summarizer's usage.
    agy_step(&mut st, "conv-main", 2, "checkpoint", Some((9_000, 170_000, 300)));
    assert!(st.compacted);
    assert_eq!(st.context.as_ref().map(|c| c.used), Some(180_500));

    // Post-compaction agent_response updates live context to the smaller size.
    agy_step(&mut st, "conv-main", 3, "agent_response", Some((12_000, 0, 200)));
    assert_eq!(st.context.as_ref().map(|c| c.used), Some(12_200));

    // A resumed call answered in a different conversation clears session signals in settle().
    let argv: Vec<String> = ["agy", "--conversation", "conv-expected"].into_iter().map(str::to_owned).collect();
    agy::settle(&argv, &mut st, "");
    assert!(!st.answered && !st.compacted && st.context.is_none());
    assert!(matches!(st.result, Some(Err(_))));
}

#[test]
fn fac_692_codex_argv_disables_unused_instructions_and_features() {
    let seat = config("[seats.foreman]\ncli = \"codex\"\nmodel = \"gpt-6.2-sol\"\neffort = \"high\"\n")
        .seat("foreman")
        .unwrap();
    let expected = [
        "sandbox_mode=danger-full-access",
        "approval_policy=never",
        "allow_login_shell=false",
        "features.apps=false",
        "features.default_mode_request_user_input=false",
        "include_permissions_instructions=false",
        "include_apps_instructions=false",
        "include_collaboration_mode_instructions=false",
        "features.plugins=false",
        "agents.enabled=false",
        "features.multi_agent=false",
        "features.multi_agent_v2=false",
    ];
    let cases = [
        codex::argv(&seat, None, "/tmp/io", &[], &[], None),
        codex::argv(&seat, Some("thread-1"), "/tmp/io", &[], &[], None),
        codex::app_server_argv(&seat, &[], None),
    ];
    for argv in cases {
        for setting in expected {
            assert!(argv.windows(2).any(|w| w[0] == "-c" && w[1] == setting), "missing -c {setting} in {argv:?}");
        }
    }
}
