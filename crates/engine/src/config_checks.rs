// Value checks for config keys added in fac-721, kept beside config.rs to stay under file caps.

use std::collections::BTreeMap;

use mork_types::config::Config;

use crate::config::{ConfigError, key_line, table_line};

pub(crate) fn check(text: &str, config: &Config) -> Result<(), ConfigError> {
    check_skills(text, config)?;
    check_claude_accounts(text, config)?;
    check_domains(text, "mork", &config.mork.domains)?;
    for (name, project) in &config.projects {
        check_domains(text, &format!("projects.{name}"), &project.domains)?;
    }
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
    }
    Ok(())
}

fn check_projects(text: &str, config: &Config) -> Result<(), ConfigError> {
    for (name, p) in &config.projects {
        if let mork_types::config::ForgeConfig::Github { token, approver, .. } = &p.forge {
            let table = format!("projects.{name}.forge");
            if !std::path::Path::new(token).is_absolute() {
                return Err(ConfigError::Value {
                    line: key_line(text, Some(&table), "token"),
                    message: format!("project {name} forge.token must be an absolute path"),
                });
            }
            if p.forge.merges_on_approval() && approver.as_deref().is_none_or(|s| s.trim().is_empty()) {
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
