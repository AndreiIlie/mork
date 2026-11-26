// fac-721: config validation for new keys and skills folder materialization.

// why: test code: a failed unwrap is the test failing.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use mork_engine::builds::skills::materialize_into;
use mork_engine::config::{ConfigError, parse};

const BASE: &str = r#"
[mork]
project = "mork"

[git]
author = "Test <test@example.invalid>"

[projects."example.com"]
repo = "/r/example"
board = { kind = "markdown", path = "/b/example" }
test = "npm test"
"#;

fn parse_with(extra: &str) -> Result<mork_types::config::Config, ConfigError> {
    parse(&format!("{BASE}\n{extra}"))
}

#[test]
fn config_parse_accepts_all_configured_tables_and_values() {
    let text = r#"
[mork]
project = "mork"
skills = "/srv/mork/skills"

[git]
author = "Test <test@example.invalid>"

[claude_accounts]
personal = "Claude · personal"
work-1 = "Claude · work"

[projects."example.com"]
repo = "/r/example"
board = { kind = "markdown", path = "/b/example" }
test = "npm test"
[projects."example.com".forge]
kind = "bitbucket"
workspace = "ws"
slug = "repo"
review_tag = "@review-bot"

[projects."example.com".tickets]
kind = "azure-devops"
token = "/credentials/tickets"
api = "https://example.invalid/org/proj/_apis/wit/workitems"
page = "https://example.invalid/org/proj/_workitems/edit/"

[tunnels.production]
alias = "ops@prod-box"
port = 4000
remote_port = 9000
url = "http://localhost:4000"

[tunnels.staging]
alias = "staging-box"
port = 4001
remote_port = 9000
url = "http://localhost:4001"
"#;
    let c = parse(text).unwrap();
    assert_eq!(c.mork.skills.as_deref(), Some("/srv/mork/skills"));
    assert_eq!(c.claude_accounts.get("personal").map(String::as_str), Some("Claude · personal"));
    assert_eq!(c.claude_accounts.get("work-1").map(String::as_str), Some("Claude · work"));
    let p = &c.projects["example.com"];
    assert!(p.forge.has_review_trigger());
    assert!(!p.forge.merges_on_approval());
    let t = p.tickets.as_ref().unwrap();
    assert_eq!(t.api(), "https://example.invalid/org/proj/_apis/wit/workitems");
    assert_eq!(t.page(), "https://example.invalid/org/proj/_workitems/edit/");
    assert_eq!(c.tunnels.keys().cloned().collect::<Vec<_>>(), vec!["production", "staging"]);
    assert_eq!(c.tunnels["production"].alias, "ops@prod-box");
    assert_eq!(c.tunnels["staging"].port, 4001);
}

const VALUE_CASES: &[(&str, usize, &str)] = &[
    ("[mork]\nproject = \"mork\"\nskills = \"relative/path\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n", 3, "skills"),
    ("[mork]\nproject = \"mork\"\nskills = \"/srv/../skills\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n", 3, "skills"),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[claude_accounts]\n\"bad/name\" = \"Header\"\n",
        8,
        "invalid Claude account name",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[claude_accounts]\n\"..\" = \"Header\"\n",
        8,
        "invalid Claude account name",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[claude_accounts]\npersonal = \"   \"\n",
        8,
        "non-empty header",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[projects.ex]\nrepo = \"/r\"\nboard = { kind = \"markdown\", path = \"/b\" }\ntest = \"t\"\n[projects.ex.forge]\nkind = \"bitbucket\"\nworkspace = \"ws\"\nslug = \"ex\"\nreview_tag = \"  \"\n",
        15,
        "review_tag",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[projects.ex]\nrepo = \"/r\"\nboard = { kind = \"markdown\", path = \"/b\" }\ntest = \"t\"\n\n[projects.ex.tickets]\nkind = \"azure-devops\"\ntoken = \"/cred\"\napi = \"http://not-https.invalid\"\npage = \"https://example.invalid/\"\n",
        15,
        "tickets.api",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[projects.ex]\nrepo = \"/r\"\nboard = { kind = \"markdown\", path = \"/b\" }\ntest = \"t\"\n\n[projects.ex.tickets]\nkind = \"azure-devops\"\ntoken = \"/cred\"\napi = \"https://example.invalid/api/\"\npage = \"https://example.invalid/\"\n",
        15,
        "tickets.api",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[projects.ex]\nrepo = \"/r\"\nboard = { kind = \"markdown\", path = \"/b\" }\ntest = \"t\"\n\n[projects.ex.tickets]\nkind = \"azure-devops\"\ntoken = \"/cred\"\napi = \"https://example.invalid/api\"\npage = \"ftp://example.invalid/\"\n",
        16,
        "tickets.page",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.\"bad name\"]\nalias = \"box\"\nport = 4000\nremote_port = 9000\nurl = \"http://localhost:4000\"\n",
        7,
        "tunnel name",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.prod]\nalias = \"-oProxyCommand=bad\"\nport = 4000\nremote_port = 9000\nurl = \"http://localhost:4000\"\n",
        8,
        "alias",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.prod]\nalias = \"prod-box\"\nport = 0\nremote_port = 9000\nurl = \"http://localhost:4000\"\n",
        9,
        "port",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.a]\nalias = \"a-box\"\nport = 4000\nremote_port = 9000\nurl = \"http://localhost:4000\"\n\n[tunnels.b]\nalias = \"b-box\"\nport = 4000\nremote_port = 9000\nurl = \"http://localhost:4001\"\n",
        15,
        "shares local port",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.prod]\nalias = \"prod-box\"\nport = 4000\nremote_port = 0\nurl = \"http://localhost:4000\"\n",
        10,
        "remote_port",
    ),
    (
        "[mork]\nproject = \"mork\"\n\n[git]\nauthor = \"T <t@e.invalid>\"\n\n[tunnels.prod]\nalias = \"prod-box\"\nport = 4000\nremote_port = 9000\nurl = \"localhost:4000\"\n",
        11,
        "url",
    ),
];

#[test]
fn config_parse_rejects_non_tables_and_invalid_values_with_lines() {
    assert!(matches!(parse(&format!("claude_accounts = \"bad\"\n{BASE}")), Err(ConfigError::NotATable { .. })));
    assert!(matches!(parse(&format!("tunnels = \"bad\"\n{BASE}")), Err(ConfigError::NotATable { .. })));
    assert!(matches!(parse_with("[tunnels]\nproduction = \"bad\"\n"), Err(ConfigError::NotATable { .. })));
    assert!(matches!(parse_with("tickets = \"bad\"\n"), Err(ConfigError::NotATable { .. })));
    for (toml, want_line, want_msg) in VALUE_CASES {
        let err = parse(toml).expect_err(&format!("expected error for {want_msg}"));
        match err {
            ConfigError::Value { line, message } => {
                assert_eq!(line, *want_line, "wrong line for {want_msg}: {message}");
                assert!(message.contains(want_msg), "expected `{want_msg}` in `{message}`");
            }
            other => panic!("expected ConfigError::Value for {want_msg}, got {other:?}"),
        }
    }
}

fn fresh_root(tag: &str) -> PathBuf {
    let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let dir = base.join(format!("mork-test-skills-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn skills_materialize_merges_skips_links_and_refuses_bad_roots_and_collisions() {
    let root = fresh_root("merge");
    let out = root.join("out");

    // Missing extra folder falls back to built-in skills alone.
    let builtin_only = materialize_into(&out, None).unwrap().unwrap();
    let missing_dir = materialize_into(&out, Some(&root.join("does-not-exist"))).unwrap().unwrap();
    assert_eq!(builtin_only, missing_dir);
    assert!(builtin_only.join("commands/SKILL.md").is_file());

    // Valid extra folder merges custom skill and skips symlinked files/dirs inside it.
    let extra = root.join("extra");
    let secret = root.join("secret.txt");
    std::fs::write(&secret, "SECRET").unwrap();
    std::fs::create_dir_all(extra.join("custom-skill")).unwrap();
    std::fs::write(extra.join("custom-skill/SKILL.md"), "# Custom skill\n").unwrap();
    symlink(&secret, extra.join("custom-skill/leak.txt")).unwrap();
    symlink(&root, extra.join("linked-dir")).unwrap();
    symlink(root.join("missing-target"), extra.join(".claude-plugin")).unwrap();

    let merged = materialize_into(&out, Some(&extra)).unwrap().unwrap();
    assert_ne!(merged, builtin_only);
    assert_eq!(std::fs::read_to_string(merged.join("custom-skill/SKILL.md")).unwrap(), "# Custom skill\n");
    assert!(!merged.join("custom-skill/leak.txt").exists());
    assert!(!merged.join("linked-dir").exists());
    assert!(merged.join(".claude-plugin/plugin.json").is_file());
    assert!(merged.join("commands/SKILL.md").is_file());

    // Symlink root fails startup.
    let link_root = root.join("extra-link");
    symlink(&extra, &link_root).unwrap();
    assert!(materialize_into(&out, Some(&link_root)).is_err());

    // File or directory under .claude-plugin fails startup.
    let plugin_extra = root.join("plugin-extra");
    std::fs::create_dir_all(plugin_extra.join(".claude-plugin")).unwrap();
    std::fs::write(plugin_extra.join(".claude-plugin/plugin.json"), "{}").unwrap();
    let err = materialize_into(&out, Some(&plugin_extra)).unwrap_err();
    assert!(err.to_string().contains(".claude-plugin"), "{err}");

    let plugin_file_extra = root.join("plugin-file-extra");
    std::fs::create_dir_all(&plugin_file_extra).unwrap();
    std::fs::write(plugin_file_extra.join(".claude-plugin"), "{}").unwrap();
    let err = materialize_into(&out, Some(&plugin_file_extra)).unwrap_err();
    assert!(err.to_string().contains(".claude-plugin"), "{err}");

    // Collision with a built-in skill path fails startup.
    let collide_extra = root.join("collide-extra");
    std::fs::create_dir_all(collide_extra.join("commands")).unwrap();
    std::fs::write(collide_extra.join("commands/SKILL.md"), "override").unwrap();
    let err = materialize_into(&out, Some(&collide_extra)).unwrap_err();
    assert!(err.to_string().contains("collides"), "{err}");

    cleanup(&root);
}

#[test]
fn provider_tables_are_strict() {
    let project = "example.com";
    let cfg = parse(BASE).unwrap();
    assert!(!cfg.projects[project].forge.hosted());
    let cfg = parse_with(r#"forge = { kind = "local", backup = "origin" }"#).unwrap();
    assert_eq!(cfg.projects[project].forge.backup().map(String::as_str), Some("origin"));
    for extra in [
        r#"bitbucket = { workspace = "ws", slug = "ex" }"#,
        r#"review_tag = "tag""#,
        r#"backup = "origin""#,
        r#"forge = { kind = "local", workspace = "ws" }"#,
        r#"tickets = { api = "https://a", page = "https://p" }"#,
        r#"tickets = { kind = "azure-devops", api = "https://a", page = "https://p" }"#,
        r#"tickets = { kind = "azure-devops", api = "https://a", page = "https://p", token = "relative" }"#,
    ] {
        assert!(parse_with(extra).is_err(), "{extra}");
    }
    assert!(parse(&BASE.replace(r#"{ kind = "markdown", path = "/b/example" }"#, r#""/b/example""#)).is_err());
}

#[test]
fn github_forge_config_checks_credentials_and_merge_mode() {
    let config = |token: &str, extra: &str| {
        parse_with(&format!(
            r#"[projects."example.com".forge]
kind = "github"
owner = "example"
repo = "repo"
token = "{token}"
{extra}"#
        ))
    };
    let manual = config("/token", "").unwrap();
    let forge = &manual.projects["example.com"].forge;
    assert!(!forge.merges_on_approval());
    assert_eq!(forge.repo_key().as_deref(), Some("github.com/example/repo"));
    assert!(config("relative", "").is_err());
    assert!(config("/token", r#"review_tag = " ""#).is_err());
    assert!(config("/token", r#"merge = "approval""#).is_err());
    let approved = config("/token", "merge = \"approval\"\napprover = \"operator\"").unwrap();
    assert!(approved.projects["example.com"].forge.merges_on_approval());
}
