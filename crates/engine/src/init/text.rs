use std::path::Path;

use mork_types::config::{Cli, SEAT_DEFAULTS};
use serde::Serialize;

use super::detect::Tools;

fn quote(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

fn path_text(p: &Path) -> Result<&str, String> {
    p.to_str().ok_or_else(|| format!("path is not valid UTF-8: {}", p.display()))
}

fn domains(cli: Cli) -> &'static [&'static str] {
    match cli {
        Cli::Claude => &["api.anthropic.com", "platform.claude.com"],
        Cli::Codex => &["chatgpt.com", "auth.openai.com"],
        Cli::Agy => &[
            "cloudcode-pa.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "aicode.googleapis.com",
            "oauth2.googleapis.com",
            "www.googleapis.com",
        ],
    }
}

pub(super) fn global(tools: &Tools, bin_tools: &Path, assets: Option<&Path>, author: &str) -> Result<String, String> {
    let mut mounts = Vec::new();
    for path in &tools.mounts {
        mounts.push(format!("{{ path = {} }}", quote(path_text(path)?)));
    }
    if !tools.links.is_empty() {
        mounts.push(format!("{{ path = {}, bin = \"bin\" }}", quote(path_text(bin_tools)?)));
    }
    let hosts: Vec<_> = tools.clis.iter().flat_map(|cli| domains(*cli)).map(|s| quote(s)).collect();
    let mut out =
        format!("[mork]\nweb_port = 41900\ndomains = [{}]\ntoolchains = [{}]\n", hosts.join(", "), mounts.join(", "));
    if let Some(assets) = assets {
        out.push_str(&format!("web_assets = {}\n", quote(path_text(assets)?)));
    }
    if let Some(root) = &tools.rustup {
        out.push_str(&format!("env = {{ RUSTUP_HOME = {} }}\n", quote(path_text(root)?)));
    }
    let mut caches = Vec::new();
    if tools.cargo {
        caches.push("\".cargo/registry\"");
        out.push_str("cache_files = [\".cargo/.package-cache\", \".cargo/.package-cache-mutate\"]\n");
    }
    if tools.npm {
        caches.push("\".npm/_cacache\"");
    }
    if !caches.is_empty() {
        out.push_str(&format!("caches = [{}]\n", caches.join(", ")));
    }
    out.push_str(&format!("\n[git]\nauthor = {}\n", quote(author)));
    if !(tools.clis.contains(&Cli::Claude) && tools.clis.contains(&Cli::Codex)) {
        let cli = tools.clis.first().ok_or("no coding CLI on PATH")?.as_str();
        for seat in SEAT_DEFAULTS.iter().map(|s| s.0).filter(|name| *name != "echo") {
            out.push_str(&format!("\n[seats.{seat}]\ncli = {}\n", quote(cli)));
        }
    }
    Ok(out)
}

pub(super) fn name(root: &Path, given: Option<&str>) -> Result<String, String> {
    let name = given.or_else(|| root.file_name()?.to_str()).unwrap_or("");
    if !name.is_empty()
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        Ok(name.into())
    } else {
        Err("pass --name with a project name using letters, digits, dots, underscores or dashes".into())
    }
}

pub(super) fn valid_prefix(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

pub(super) fn prefix(name: &str, given: Option<&str>) -> Result<String, String> {
    let derived: String =
        name.chars().filter(char::is_ascii_alphabetic).take(3).map(|c| c.to_ascii_lowercase()).collect();
    let prefix = given.unwrap_or(&derived);
    if valid_prefix(prefix) { Ok(prefix.into()) } else { Err("pass --prefix with one or more ASCII letters".into()) }
}

pub(super) fn test_command<'a>(given: Option<&'a str>, cargo: bool, package: Option<&str>) -> Result<&'a str, String> {
    let npm = || {
        let json: serde_json::Value = serde_json::from_str(package?).ok()?;
        json["scripts"]["test"].as_str().filter(|s| !s.trim().is_empty()).map(|_| "npm test")
    };
    given
        .or_else(|| cargo.then_some("cargo test"))
        .or_else(npm)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "no test command found: pass --test '<command>'".into())
}

#[derive(Serialize)]
pub(super) struct Project<'a> {
    pub repo: &'a Path,
    pub base: &'a str,
    pub test: &'a str,
    pub install: Option<&'static str>,
    pub domains: Vec<&'static str>,
}

pub(super) fn add_project(old: &str, name: &str, board: &Path, project: &Project<'_>) -> Result<String, String> {
    let mut out = old.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "\n[projects.{}]\nboard = {{ kind = \"markdown\", path = {} }}\n",
        quote(name),
        quote(path_text(board)?)
    ));
    out.push_str(&toml::to_string(project).map_err(|e| e.to_string())?);
    Ok(out)
}

pub(super) fn board(name: &str, prefix: &str) -> String {
    format!("project_name: \"{name}\"\ntask_prefix: \"{prefix}\"\nstatuses: [\"Backlog\", \"In Progress\", \"Done\"]\n")
}

pub(super) fn destinations_outside(repo: &Path, destinations: &[&Path]) -> Result<(), String> {
    if let Some(path) = destinations.iter().find(|p| p.starts_with(repo)) {
        return Err(format!("{} would be written inside the repository", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use mork_types::config::Standing;

    #[test]
    fn every_seat_runs_an_installed_cli() -> Result<(), String> {
        for mask in 1..8 {
            let clis = [Cli::Claude, Cli::Codex, Cli::Agy]
                .into_iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, c)| c)
                .collect();
            let tools = Tools { clis, ..Tools::default() };
            let cfg = config::parse(&global(&tools, Path::new("/tools"), None, "A <a@b>")?)?;
            for seat in SEAT_DEFAULTS.iter().map(|s| s.0).filter(|name| *name != "echo") {
                for at in Standing::ALL {
                    assert!(tools.clis.contains(&cfg.seat_in(seat, at).unwrap().cli));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn project_after_example_preserves_its_text_and_paths() -> Result<(), String> {
        let example = include_str!("../../../../mork.example.toml");
        let repo = Path::new("/work/space \"and quote");
        let mut project =
            Project { repo, base: "work", test: "npm test", install: None, domains: vec!["registry.npmjs.org"] };
        for install in [None, Some("npm ci")] {
            project.install = install;
            let full = add_project(example, "my.app", Path::new("/boards/my.app"), &project)?;
            assert!(full.starts_with(example));
            let cfg = config::parse(&full)?;
            let p = &cfg.projects["my.app"];
            assert_eq!(p.repo, repo.to_str().unwrap());
            assert_eq!(p.test, "npm test");
            assert_eq!(p.install.as_deref(), install);
        }
        Ok(())
    }

    #[test]
    fn test_and_prefix_detection() -> Result<(), String> {
        let npm = Some(r#"{"scripts":{"test":"jest"}}"#);
        assert_eq!(test_command(None, true, npm)?, "cargo test");
        assert_eq!(test_command(None, false, npm)?, "npm test");
        assert_eq!(test_command(Some("make check"), false, None)?, "make check");
        assert!(test_command(None, false, Some("{}")).is_err());
        assert!(test_command(None, false, None).is_err());
        assert_eq!(prefix("2-My.App", None)?, "mya");
        assert!(!valid_prefix("app2"));
        let repo = Path::new("/mork/boards/app");
        assert!(destinations_outside(repo, &[repo]).is_err());
        assert!(destinations_outside(repo, &[&repo.join("backlog")]).is_err());
        destinations_outside(repo, &[Path::new("/mork/boards/other")])
    }
}
