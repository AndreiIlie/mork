// Value checks for config keys added in fac-721, kept beside config.rs to stay under file caps.

use std::collections::BTreeMap;

use mork_types::config::Config;

use crate::config::{ConfigError, key_line, table_line};

pub(crate) fn check(text: &str, config: &mut Config) -> Result<(), ConfigError> {
    check_skills(text, config)?;
    check_claude_accounts(text, config)?;
    check_domains(text, "mork", &config.mork.domains)?;
    for (name, project) in &config.projects {
        let table = format!("projects.{name}");
        check_domains(text, &table, &project.domains)?;
        if let Some(range) = project.host_ports.iter().find(|r| r.from < 1024) {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "host_ports"),
                message: format!("project `{name}` host_ports includes port {}, below the minimum of 1024", range.from),
            });
        }
    }
    check_pass_pairs(text, config)?;
    check_projects(text, config)?;
    check_tunnels(text, config)?;
    Ok(())
}

fn has_ws_or_ctrl(s: &str) -> bool {
    s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn web_url(s: &str) -> bool {
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty()) && !has_ws_or_ctrl(s)
}

fn check_skills(text: &str, config: &Config) -> Result<(), ConfigError> {
    let Some(skills) = config.mork.skills.as_deref() else { return Ok(()) };
    let ok = skills.starts_with('/') && skills.split('/').all(|c| c != "." && c != "..");
    if !ok {
        return Err(ConfigError::Value {
            line: key_line(text, Some("mork"), "skills"),
            message: "`skills` must be an absolute path with no `.` or `..` component".into(),
        });
    }
    Ok(())
}

fn check_claude_accounts(text: &str, config: &Config) -> Result<(), ConfigError> {
    let mut headers = BTreeMap::new();
    for (name, header) in &config.claude_accounts {
        let valid_name = !name.is_empty()
            && name != "."
            && name != ".."
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !valid_name {
            return Err(ConfigError::Value {
                line: key_line(text, Some("claude_accounts"), name),
                message: format!("invalid Claude account name `{name}`: use `[A-Za-z0-9._-]`, not `.` or `..`"),
            });
        }
        if header.trim().is_empty() {
            return Err(ConfigError::Value {
                line: key_line(text, Some("claude_accounts"), name),
                message: format!("Claude account `{name}` must have a non-empty header"),
            });
        }
        if matches!(header.as_str(), "codex" | "agy gemini" | "agy 3p") {
            return Err(ConfigError::Value {
                line: key_line(text, Some("claude_accounts"), name),
                message: format!("Claude account `{name}` uses the built-in pool header `{header}`"),
            });
        }
        if let Some(other) = headers.insert(header, name) {
            return Err(ConfigError::Value {
                line: key_line(text, Some("claude_accounts"), name),
                message: format!("Claude accounts `{other}` and `{name}` share the pool header `{header}`"),
            });
        }
    }
    Ok(())
}

fn check_pass_pairs(text: &str, config: &Config) -> Result<(), ConfigError> {
    use mork_types::config::{MODEL_CATALOG, OPERATOR_MODELS, bench::benchmark};
    let Some(picks) = &config.picks else { return Ok(()) };
    for entry in &picks.pass {
        let model = &entry.model;
        let cli = entry.cli.as_str();
        let invalid = |why: String| ConfigError::Value {
            line: table_line(text, "picks.pass"),
            message: format!("picks.pass {cli} {model}: {why}"),
        };
        if !MODEL_CATALOG.iter().chain(OPERATOR_MODELS).any(|&(c, id, _)| c == entry.cli && id == model) {
            return Err(invalid("unknown model on this CLI".into()));
        }
        for &effort in &entry.efforts {
            if !entry.cli.takes(model, effort) {
                return Err(invalid(format!("unsupported effort {}", effort.as_str())));
            }
            if benchmark(model, effort).is_none() {
                return Err(invalid(format!("no benchmark at {}", effort.as_str())));
            }
        }
    }
    Ok(())
}

fn check_passes(
    text: &str,
    name: &str,
    project: &mork_types::config::ProjectConfig,
    has_pairs: bool,
) -> Result<(), ConfigError> {
    use mork_types::lookback::{Read, Trigger};
    for (read, pass) in &project.passes {
        let thresholds = [pass.landings, pass.edits, pass.notes, pass.days];
        let problem = if pass.mode == Trigger::Auto && thresholds.iter().any(Option::is_some) {
            "auto takes no thresholds"
        } else if pass.mode == Trigger::Auto && !has_pairs {
            "auto requires a model and effort in picks.pass"
        } else if thresholds.contains(&Some(0)) {
            "thresholds must be positive"
        } else if pass.mode == Trigger::Scheduled && thresholds.iter().all(Option::is_none) {
            "scheduled requires a threshold"
        } else if *read != Read::Dream && pass.budget.is_some() {
            "only dream takes a budget"
        } else {
            continue;
        };
        let table = format!("projects.{name}.passes.{}", read.spec().name);
        return Err(ConfigError::Value { line: table_line(text, &table), message: format!("{table}: {problem}") });
    }
    Ok(())
}

fn check_projects(text: &str, config: &mut Config) -> Result<(), ConfigError> {
    let has_pairs = config.picks.as_ref().is_some_and(|p| p.pass.iter().any(|m| !m.efforts.is_empty()));
    for (name, p) in &mut config.projects {
        check_passes(text, name.as_str(), p, has_pairs)?;
        let approval = p.forge.merges_on_approval();
        if let mork_types::config::ForgeConfig::Github { token, approver, .. } = &mut p.forge {
            let table = format!("projects.{name}.forge");
            if !std::path::Path::new(token).is_absolute() {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "token"),
                    message: format!("project {name} forge.token must be an absolute path"),
                });
            }
            *approver = approver.as_deref().map(|s| s.trim().to_owned());
            if approval && approver.as_deref().is_none_or(str::is_empty) {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "merge"),
                    message: format!("project {name} forge.approver is required for approval merges"),
                });
            }
        }
        if p.forge.review_tag().is_some_and(|tag| tag.trim().is_empty()) {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&format!("projects.{name}.forge")), "review_tag"),
                message: format!("project `{name}` `review_tag` must not be empty"),
            });
        }
        if let Some(t) = &p.tickets {
            let table = format!("projects.{name}.tickets");
            if !std::path::Path::new(t.token()).is_absolute() {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "token"),
                    message: format!("project {name} tickets.token must be an absolute path"),
                });
            }
            let api_ok = t.api().starts_with("https://")
                && t.api().len() > "https://".len()
                && !t.api().ends_with('/')
                && !has_ws_or_ctrl(t.api());
            if !api_ok {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "api"),
                    message: format!(
                        "project `{name}` `tickets.api` must be an `https://` URL with no trailing `/` or whitespace"
                    ),
                });
            }
            if !web_url(t.page()) {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "page"),
                    message: format!(
                        "project `{name}` `tickets.page` must be an `http://` or `https://` URL with no whitespace"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn check_tunnels(text: &str, config: &Config) -> Result<(), ConfigError> {
    let mut ports: BTreeMap<u16, &str> = BTreeMap::new();
    for (name, t) in &config.tunnels {
        let table = format!("tunnels.{name}");
        if name.is_empty() || has_ws_or_ctrl(name) {
            return Err(ConfigError::Value {
                line: table_line(text, &table),
                message: format!("tunnel name `{name}` must not be empty or hold whitespace"),
            });
        }
        if t.alias.is_empty() || t.alias.starts_with('-') || has_ws_or_ctrl(&t.alias) {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "alias"),
                message: format!(
                    "tunnel `{name}` `alias` must be non-empty, not start with `-`, and hold no whitespace"
                ),
            });
        }
        if t.port == 0 {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "port"),
                message: format!("tunnel `{name}` `port` must be non-zero"),
            });
        }
        if let Some(other) = ports.insert(t.port, name.as_str()) {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "port"),
                message: format!("tunnel `{name}` shares local port {} with `{other}`", t.port),
            });
        }
        if t.remote_port == 0 {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "remote_port"),
                message: format!("tunnel `{name}` `remote_port` must be non-zero"),
            });
        }
        if !web_url(&t.url) {
            return Err(ConfigError::Value {
                line: key_line(text, Some(&table), "url"),
                message: format!("tunnel `{name}` `url` must be an `http://` or `https://` URL with no whitespace"),
            });
        }
    }
    Ok(())
}

pub(crate) fn caches(text: &str, config: &mut mork_types::config::MorkConfig) -> Result<(), ConfigError> {
    use std::path::Path;
    let folders = &config.caches;
    for (key, entries) in [("caches", folders), ("cache_files", &config.cache_files)] {
        for name in entries {
            let invalid = name.split('/').any(|p| matches!(p, "" | "." | "..") || p.contains('\0'));
            let under_file =
                Path::new(name).ancestors().skip(1).any(|p| config.cache_files.iter().any(|f| Path::new(f) == p));
            if invalid || under_file || (key == "cache_files" && folders.contains(name)) {
                return Err(ConfigError::Value {
                    line: key_line(text, Some("mork"), key),
                    message: format!("invalid or conflicting {key} entry: {name:?}"),
                });
            }
        }
    }
    let folders = folders.clone();
    for entries in [&mut config.cache_files, &mut config.caches] {
        entries
            .retain(|entry| !Path::new(entry).ancestors().skip(1).any(|p| folders.iter().any(|f| Path::new(f) == p)));
        entries.sort();
        entries.dedup();
    }
    Ok(())
}

fn check_domains(text: &str, table: &str, entries: &[String]) -> Result<(), ConfigError> {
    for entry in entries {
        if destination(entry, Some(443)).is_none() {
            return Err(ConfigError::Value {
                line: key_line(text, Some(table), "domains"),
                message: format!("invalid domains entry {entry:?} in {table}: expected host or host:port"),
            });
        }
    }
    Ok(())
}

pub(crate) fn destination(entry: &str, default: Option<u16>) -> Option<(&str, u16)> {
    let (host, suffix) = if let Some(rest) = entry.strip_prefix('[') {
        let (ip, port) = rest.split_once(']')?;
        ip.parse::<std::net::Ipv6Addr>().ok()?;
        (ip, port)
    } else {
        let cut = entry.find(':').unwrap_or(entry.len());
        let (host, suffix) = entry.split_at(cut);
        if !hostname(host) {
            return None;
        }
        (host, suffix)
    };
    if suffix.is_empty() {
        return Some((host, default?));
    }
    let digits = suffix.strip_prefix(':')?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((host, digits.parse::<u16>().ok().filter(|p| *p > 0)?))
}

fn hostname(host: &str) -> bool {
    let name = host.strip_suffix('.').unwrap_or(host);
    name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod domain_tests {
    use super::*;

    #[test]
    fn validates_configured_domains() {
        let valid = ["registry.example", "registry.example:8443", "[::1]:443"].map(str::to_owned);
        assert!(check_domains("", "mork", &valid).is_ok());
        for bad in ["", "bad host", "*.example", "name:0", "name:65536", "a/b", "https://name", "[bad]"] {
            assert!(check_domains("", "mork", &[bad.to_owned()]).is_err(), "{bad}");
        }
    }
}
