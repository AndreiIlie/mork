use super::*;
use serde_json::json;
use std::os::unix::fs::{PermissionsExt, symlink};

const BASE: &str = "[mork]\n[git]\nauthor='Example'\n[projects.web]\nrepo='/repo'\ntest='test'\nboard={kind='markdown',path='/board'}\n";
fn change(group: &str, key: &str, old: serde_json::Value, new: serde_json::Value) -> ConfigChange {
    serde_json::from_value(json!({"section":group,"project":(group=="project").then_some("web"),
        "fields":{key:{"read":old,"value":new}}}))
    .unwrap()
}

#[test]
fn edit_only_named_fields_even_with_dotted_keys() {
    let text = "mork.max_test_runs = 4 # keep this\ngit.author='Example'\nmork.context_cap=400000";
    let cfg = config::parse(text).unwrap();
    let edit = change("limits", "max_test_runs", json!({"text":"4"}), json!({"text":"5"}));
    let (s, _) = candidate(text, &cfg, &edit, &Secrets::default()).unwrap();
    assert_eq!(s, text.replacen("= 4", "= 5", 1));
    let reset = change("limits", "context_cap", json!({"text":"400000"}), json!({"text":"200000"}));
    assert!(!candidate(text, &cfg, &reset, &Secrets::default()).unwrap().0.contains("context_cap"));
    let sole = "mork.context_cap=400000\ngit.author='Example'";
    let (s, _) = candidate(sole, &config::parse(sole).unwrap(), &reset, &Secrets::default()).unwrap();
    assert!(!s.contains("context_cap"));
}

#[test]
fn adds_an_absent_setting_and_keeps_explicit_empty_ports() {
    let cfg = config::parse(BASE).unwrap();
    let set = change("reach", "ports", json!({"entries":["443","80","53"]}), json!({"entries":[]}));
    let (text, cfg) = candidate(BASE, &cfg, &set, &Secrets::default()).unwrap();
    assert!(cfg.mork.ports.is_empty());
    let new = change("project", "passes.dream.budget", json!({"text":"60"}), json!({"text":"72"}));
    let (_, cfg) = candidate(&text, &cfg, &new, &Secrets::default()).unwrap();
    assert_eq!(cfg.projects["web"].passes[&mork_types::lookback::Read::Dream].budget, Some(72));
}

#[test]
fn only_auto_clears_the_pass_thresholds_and_keeps_its_budget() {
    use mork_types::lookback::Read;
    let text = format!(
        "{BASE}[projects.web.passes.dream]\nmode='scheduled'\nlandings=5\nedits=6\nnotes=7\ndays=8\nbudget=9\n\
         [[picks.pass]]\ncli='codex'\nmodel='gpt-6.2-sol'\nlabel='Sol'\nefforts=['high']\n"
    );
    let cfg = config::parse(&text).unwrap();
    let mode = |from: &str, to: &str| change("project", "passes.dream.mode", json!({"text":from}), json!({"text":to}));
    let (text, cfg) = candidate(&text, &cfg, &mode("scheduled", "manual"), &Secrets::default()).unwrap();
    let pass = &cfg.projects["web"].passes[&Read::Dream];
    assert_eq!([pass.landings, pass.edits, pass.notes, pass.days], [Some(5), Some(6), Some(7), Some(8)]);
    let (_, cfg) = candidate(&text, &cfg, &mode("manual", "auto"), &Secrets::default()).unwrap();
    let pass = &cfg.projects["web"].passes[&Read::Dream];
    assert_eq!([pass.landings, pass.edits, pass.notes, pass.days], [None; 4]);
    assert_eq!(pass.budget, Some(9));
}

#[test]
fn picks_keep_their_label_and_validate_new_offers() {
    let text = format!(
        "{BASE}[[picks.researcher]]\ncli='codex'\nmodel='gpt-6.2-sol'\nlabel='My Sol' # local alias\nefforts=['high']\n"
    );
    let cfg = config::parse(&text).unwrap();
    let picks = &cfg.picks.as_ref().unwrap().researcher;
    let mut new = picks.clone();
    new[0].efforts.push(mork_types::config::Effort::Xhigh);
    let edit = change("picks", "researcher", json!({"picks":picks}), json!({"picks":new}));
    let (s, c) = candidate(&text, &cfg, &edit, &Secrets::default()).unwrap();
    assert!(s.contains("label='My Sol' # local alias"));
    assert_eq!(c.picks.unwrap().researcher, new);
    new[0].efforts.push(mork_types::config::Effort::Low);
    let bad = change("picks", "researcher", json!({"picks":picks}), json!({"picks":new}));
    assert!(candidate(&text, &cfg, &bad, &Secrets::default()).is_err());
    let remove = change("picks", "researcher", json!({"picks":picks}), json!({"picks":[]}));
    let (s, cfg) = candidate(&text, &cfg, &remove, &Secrets::default()).unwrap();
    assert!(cfg.picks.is_none());
    assert!(s.contains("# local alias"));
}

#[test]
fn seat_reset_handles_common_and_work_overrides() {
    use mork_types::config::{Cli, Effort, Standing};
    let text =
        format!("{BASE}[seats.foreman]\ncli='claude'\nmodel='claude-opus-5-5'\n[seats.planner.work]\neffort='low'\n");
    let mut cfg = config::parse(&text).unwrap();
    let before = cfg.seat_in("foreman", Standing::REPORT).unwrap();
    let mut text = text;
    for key in ["foreman.work", "planner.work"] {
        let view = read(&cfg, &Secrets::default(), false);
        let field = view.sections.iter().flat_map(|s| &s.fields).find(|f| f.key == key).unwrap();
        let reset = change("seats", key, json!(field.value), json!(field.default));
        (text, cfg) = candidate(&text, &cfg, &reset, &Secrets::default()).unwrap();
    }
    assert_eq!(cfg.seat("foreman").unwrap().cli, Cli::Codex);
    assert_eq!(cfg.seat("planner").unwrap().effort, Effort::Xhigh);
    assert_eq!(cfg.seat_in("foreman", Standing::REPORT).unwrap(), before);
}

#[test]
fn file_changes_keep_links_permissions_and_refuse_stale_or_invalid_updates() {
    let dir = std::env::temp_dir().join(crate::random::uuid());
    fs::create_dir(&dir).unwrap();
    let file = dir.join("real");
    let path = dir.join("mork.toml");
    fs::write(&file, BASE).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&file, &path).unwrap();
    let cfg = config::parse(BASE).unwrap();
    for (old, new) in [("3", "0"), ("4", "5")] {
        let bad = change("limits", "max_test_runs", json!({"text":old}), json!({"text":new}));
        assert!(save(&path, BASE, &cfg, &bad, &Secrets::default()).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), BASE);
    }
    let good = change("limits", "max_test_runs", json!({"text":"3"}), json!({"text":"5"}));
    let (text, c) = save(&path, BASE, &cfg, &good, &Secrets::default()).unwrap();
    assert_eq!(c.mork.max_test_runs, 5);
    assert!(path.is_symlink());
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o640);
    assert!(save(&path, BASE, &cfg, &good, &Secrets::default()).is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), text);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn view_and_refusal_withhold_configured_secrets() {
    let text = format!(
        "{BASE}copy=['hidden-copy']\nchecks={{command='check synthetic-token',folder='/checks'}}\n[seats.planner]\ncredentials='/private-creds'\nenv={{SECRET='hidden-env'}}\n[projects.web.forge]\nkind='github'\nowner='example'\nrepo='web'\ntoken='/private-token-file'\n"
    );
    let cfg = config::parse(&text).unwrap();
    let mut secrets = Secrets::default();
    secrets.add("test", "synthetic-token");
    let out = serde_json::to_string(&read(&cfg, &secrets, false)).unwrap();
    for private in ["synthetic-token", "hidden-copy", "/private-creds", "hidden-env", "/private-token-file"] {
        assert!(!out.contains(private));
    }
    let bad = change("project", "checks.command", json!({"text":"check"}), json!({"text":"other"}));
    let dir = std::env::temp_dir().join(crate::random::uuid());
    fs::create_dir(&dir).unwrap();
    let path = dir.join("mork.toml");
    fs::write(&path, &text).unwrap();
    let err = save(&path, &text, &cfg, &bad, &secrets).unwrap_err();
    assert!(err.contains("Withheld"));
    assert!(!err.contains("synthetic-token"));
    let invalid = change("reach", "domains", json!({"entries":[]}), json!({"entries":["synthetic-token/invalid"]}));
    let err = save(&path, &text, &cfg, &invalid, &secrets).unwrap_err();
    assert!(err.contains("invalid domains entry"));
    assert!(!err.contains("synthetic-token"));
    assert_eq!(fs::read_to_string(path).unwrap(), text);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn preserves_inline_project_order_and_comments_when_a_list_changes() {
    let text = "mork = { ports = [443, # HTTPS\n 80, 53], context_cap = 300000 }\ngit.author='Example'\nprojects = { web.repo='/repo', web.test='test', web.board={kind='markdown',path='/board'} }";
    let cfg = config::parse(text).unwrap();
    let set = change("reach", "ports", json!({"entries":["443","80","53"]}), json!({"entries":["80","53"]}));
    let (text, cfg) = candidate(text, &cfg, &set, &Secrets::default()).unwrap();
    assert!(text.contains("# HTTPS"));
    assert_eq!(cfg.mork.ports, [80, 53]);
    let set = change("project", "slots", json!({"text":"2"}), json!({"text":"4"}));
    let (text, cfg) = candidate(&text, &cfg, &set, &Secrets::default()).unwrap();
    assert_eq!(cfg.projects["web"].slots, 4);
    assert!(text.contains("web.repo='/repo', web.test='test', web.board={kind='markdown',path='/board'}"));
    let reset = change("project", "slots", json!({"text":"4"}), json!({"text":"2"}));
    let (text, cfg) = candidate(&text, &cfg, &reset, &Secrets::default()).unwrap();
    assert_eq!(cfg.projects["web"].slots, 2);
    assert!(!text.contains("slots"));
}
