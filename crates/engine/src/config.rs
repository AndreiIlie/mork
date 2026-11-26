//! The config file (1.14): `$MORK_CONFIG`, else `~/.config/mork/mork.toml`. Unknown keys,
//! wrong types and missing required keys are errors naming their line, or their key and table.

use std::path::{Path, PathBuf};

use mork_types::config::{
    CONTEXT_CAP_MAX, CONTEXT_CAP_MIN, Cli, Config, PROJECT_REQUIRED, REQUIRED, SEAT_DEFAULTS, SeatOverride, Standing,
    Wall, moved_to_codex,
};

/// What is wrong with the config (1.14), with its line where it has one.
#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file's text is wrong: `error` says how.
    In {
        path: PathBuf,
        error: Box<ConfigError>,
    },
    /// TOML's own error (syntax, an unknown key, a wrong type).
    Toml {
        line: Option<usize>,
        message: String,
    },
    /// A scalar where a table belongs.
    NotATable {
        line: usize,
        name: String,
        found: &'static str,
    },
    Missing {
        key: String,
        table: String,
    },
    UnknownSeat {
        line: usize,
        name: String,
    },
    /// A value TOML reads but 1.14 doesn't allow.
    Value {
        line: usize,
        message: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "config {}: {source}", path.display()),
            Self::In { path, error } => write!(f, "config {}: {error}", path.display()),
            Self::Toml { line: Some(line), message } => write!(f, "line {line}: {message}"),
            Self::Toml { line: None, message } => write!(f, "{message}"),
            Self::NotATable { line, name, found } => write!(f, "line {line}: `{name}` must be a table, not {found}"),
            Self::Missing { key, table } => write!(f, "missing key `{key}` in [{table}]"),
            Self::UnknownSeat { line, name } => write!(f, "line {line}: unknown seat `{name}`"),
            Self::Value { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<ConfigError> for String {
    fn from(e: ConfigError) -> Self {
        e.to_string()
    }
}

pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MORK_CONFIG").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(home).join(".config/mork/mork.toml"))
}

/// Reads and checks the config.
pub fn read(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read { path: path.to_owned(), source })?;
    parse(&text).map_err(|e| ConfigError::In { path: path.to_owned(), error: Box::new(e) })
}

fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset).map_or(1, |s| s.matches('\n').count() + 1)
}

pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| located(text, &e))?;
    tables_are_tables(text, &table)?;
    for (name, keys) in REQUIRED {
        let t = table.get(*name).and_then(|v| v.as_table());
        for key in *keys {
            if !t.is_some_and(|t| t.contains_key(*key)) {
                return Err(ConfigError::Missing { key: (*key).to_owned(), table: (*name).to_owned() });
            }
        }
    }
    if let Some(projects) = table.get("projects").and_then(|v| v.as_table()) {
        for (name, p) in projects {
            for key in PROJECT_REQUIRED {
                if !p.as_table().is_some_and(|t| t.contains_key(*key)) {
                    return Err(ConfigError::Missing { key: (*key).to_owned(), table: format!("projects.{name}") });
                }
            }
        }
    }
    let mut config: Config = toml::from_str(text).map_err(|e| located(text, &e))?;
    config.project_order = check_projects(text)?;
    for name in config.seats.keys() {
        if !SEAT_DEFAULTS.iter().any(|s| s.0 == name) {
            let line = text.find(&format!("[seats.{name}]")).map_or(1, |o| line_of(text, o));
            return Err(ConfigError::UnknownSeat { line, name: name.clone() });
        }
    }
    for (name, o) in &config.seats {
        let no_claude = Standing::ALL
            .into_iter()
            .filter_map(|at| config.seat_in(name, at).map(|s| (at, s)))
            .all(|(at, s)| s.cli != Cli::Claude && (!moved_to_codex(name, at) || named(o, at)));
        if o.advisor.is_some() && no_claude {
            let cli = config.seat(name).map_or("codex", |s| s.cli.as_str());
            return Err(ConfigError::Value {
                line: key_line(text, Some(&format!("seats.{name}")), "advisor"),
                message: format!("`advisor` is for Claude seats: `{name}` runs {cli}"),
            });
        }
    }
    walled_seats(text, &config)?;
    if config.mork.max_test_runs < 1 {
        return Err(ConfigError::Value {
            line: line_at(text, "max_test_runs"),
            message: "`max_test_runs` must be a whole number from 1".into(),
        });
    }
    if config.mork.context_cap != 0 && !(CONTEXT_CAP_MIN..=CONTEXT_CAP_MAX).contains(&config.mork.context_cap) {
        return Err(ConfigError::Value {
            line: key_line(text, Some("mork"), "context_cap"),
            message: format!("`context_cap` must be 0 or between {CONTEXT_CAP_MIN} and {CONTEXT_CAP_MAX}"),
        });
    }
    crate::config_checks::caches(text, &mut config.mork)?;
    slice(text, &config.slice)?;
    crate::config_checks::check(text, &config)?;
    Ok(config)
}

// The baseline seat writes a clone before any approve (fac-636), and agy approves its tools unasked (fac-657): their
// walls alone hold them.
fn walled_seats(text: &str, config: &Config) -> Result<(), ConfigError> {
    for (name, _) in config.seats.iter().filter(|(_, o)| o.wall == Some(Wall::None)) {
        let agy = Standing::ALL.into_iter().any(|at| config.seat_in(name, at).is_some_and(|s| s.cli == Cli::Agy));
        let message = if name == "baseline" {
            "`wall = \"none\"` isn't allowed on the baseline seat: it is always walled".to_owned()
        } else if agy {
            format!("`wall = \"none\"` isn't allowed on the {name} seat: it runs agy, which is always walled")
        } else {
            continue;
        };
        return Err(ConfigError::Value { line: key_line(text, Some(&format!("seats.{name}")), "wall"), message });
    }
    Ok(())
}

// A row leaving a review standing whose default moved to Codex (fac-558) loads as it did, its advisor unused there.
fn named(o: &SeatOverride, at: Standing) -> bool {
    let phase = o.at(at);
    phase.and_then(|p| p.cli).or(o.cli).is_some() || phase.and_then(|p| p.model.as_ref()).or(o.model.as_ref()).is_some()
}

// What systemd itself would refuse, refused here with its line: else every job's start fails.
fn slice(text: &str, s: &mork_types::config::SliceConfig) -> Result<(), ConfigError> {
    for (key, w) in [("cpu_weight", s.cpu_weight), ("io_weight", s.io_weight)] {
        if !(1..=10_000).contains(&w) {
            let message = format!("`{key}` must be a whole number from 1 to 10000");
            return Err(ConfigError::Value { line: key_line(text, Some("slice"), key), message });
        }
    }
    let m = s.memory_high.as_str();
    let ok = match m.strip_suffix('%') {
        Some(p) => p.parse::<u8>().is_ok_and(|p| (1..=100).contains(&p)),
        None => {
            let digits = m.strip_suffix(['K', 'M', 'G', 'T']).unwrap_or(m);
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        }
    };
    if !ok {
        let message = "`memory_high` must be a share of the RAM (`75%`) or a size (`24G`)".into();
        return Err(ConfigError::Value { line: key_line(text, Some("slice"), "memory_high"), message });
    }
    Ok(())
}

fn check_projects(text: &str) -> Result<Vec<mork_types::ProjectId>, ConfigError> {
    let table = toml::de::DeTable::parse(text).map_err(|e| located(text, &e))?;
    let projects = table.get_ref().get("projects").and_then(|v| v.get_ref().as_table());
    let mut found = Vec::new();
    for (key, value) in projects.into_iter().flat_map(|p| p.iter()) {
        let name = key.get_ref().as_ref();
        if let Some(project) = value.get_ref().as_table()
            && let Some((waits, _)) = project.get_key_value("orchestrator_waits")
            && project.contains_key("orchestrator_session")
        {
            return Err(ConfigError::Value {
                line: line_of(text, waits.span().start),
                message: format!("project `{name}` must choose either `orchestrator_waits` or `orchestrator_session`"),
            });
        }
        found.push((key.span().start, mork_types::ProjectId::new(name)));
    }
    found.sort_by_key(|(at, _)| *at);
    Ok(found.into_iter().map(|(_, n)| n).collect())
}

// The top-level tables, and each project, seat, tunnel and tickets entry, must be tables: a scalar there
// is a wrong type, named with its line, not a missing key (M1.27).
fn tables_are_tables(text: &str, table: &toml::Table) -> Result<(), ConfigError> {
    for name in REQUIRED.iter().map(|r| r.0).chain([
        "internals",
        "projects",
        "seats",
        "picks",
        "slice",
        "claude_accounts",
        "tunnels",
    ]) {
        if let Some(v) = table.get(name).filter(|v| !v.is_table()) {
            let line = key_line(text, None, name);
            return Err(ConfigError::NotATable { line, name: name.to_owned(), found: v.type_str() });
        }
    }
    for outer in ["projects", "seats", "tunnels"] {
        for (name, v) in table.get(outer).and_then(|v| v.as_table()).into_iter().flatten() {
            if !v.is_table() {
                let line = key_line(text, Some(outer), name);
                return Err(ConfigError::NotATable { line, name: format!("{outer}.{name}"), found: v.type_str() });
            }
        }
    }
    for (name, p) in table.get("projects").and_then(|v| v.as_table()).into_iter().flatten() {
        if let Some(v) = p.as_table().and_then(|t| t.get("tickets")).filter(|v| !v.is_table()) {
            let line = key_line(text, Some(&format!("projects.{name}")), "tickets");
            return Err(ConfigError::NotATable { line, name: format!("projects.{name}.tickets"), found: v.type_str() });
        }
    }
    Ok(())
}

fn clean_header(header: &str) -> String {
    header.split('.').map(|s| s.trim().trim_matches(['"', '\''])).collect::<Vec<_>>().join(".")
}

pub(crate) fn table_line(text: &str, table: &str) -> usize {
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        if let Some(h) = t.strip_prefix('[').and_then(|h| h.strip_suffix(']'))
            && clean_header(h.trim_matches(['[', ']']).trim()) == table
        {
            return i + 1;
        }
    }
    1
}

pub(crate) fn key_line(text: &str, table: Option<&str>, key: &str) -> usize {
    let mut current: Option<String> = None;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        if let Some(h) = t.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            current = Some(clean_header(h.trim_matches(['[', ']']).trim()));
            continue;
        }
        let Some((k, _)) = t.split_once('=') else { continue };
        let k = clean_header(k.trim());
        let here = current.as_deref() == table && k == key;
        let dotted = current.is_none() && table.is_some_and(|tb| k == format!("{tb}.{key}"));
        if here || dotted {
            return i + 1;
        }
    }
    1
}

fn line_at(text: &str, key: &str) -> usize {
    text.lines().position(|l| l.trim_start().starts_with(key)).map_or(1, |i| i + 1)
}

fn located(text: &str, e: &toml::de::Error) -> ConfigError {
    ConfigError::Toml { line: e.span().map(|s| line_of(text, s.start)), message: e.message().trim().to_owned() }
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, parse};

    const WAKE_PROJECT: &str = "[mork]\nproject = \"example.com\"\n[git]\nauthor = \"A <a@b>\"\n\
                               [projects.\"example.com\"]\nrepo = \"/repo\"\nboard = { kind = \"markdown\", path = \"/board\" }\ntest = \"true\"\n";

    #[test]
    fn wake_keys_are_mutually_exclusive() {
        for waits in [true, false] {
            let text = format!("{WAKE_PROJECT}orchestrator_waits = {waits}\norchestrator_session = \"session\"\n");
            let Err(ConfigError::Value { line, message }) = parse(&text) else {
                panic!("expected the wake keys to conflict");
            };
            assert_eq!(line, 9);
            assert!(message.contains("example.com"));
            assert!(message.contains("orchestrator_waits"));
            assert!(message.contains("orchestrator_session"));
        }
    }

    #[test]
    fn seat_cache_paths() -> Result<(), ConfigError> {
        let parse_lists = |lists: &str| parse(&WAKE_PROJECT.replace("[git]", &format!("{lists}\n[git]")));
        for entry in ["", ".", "p/./q", "../p", "/p"] {
            assert!(parse_lists(&format!("caches = [{entry:?}]")).is_err());
        }
        assert!(parse_lists("caches=['a']\ncache_files=['a']").is_err());
        let cfg = parse_lists("caches=['a/b','a']\ncache_files=['a/f','b/f']")?;
        assert_eq!(cfg.mork.caches, ["a"]);
        assert_eq!(cfg.mork.cache_files, ["b/f"]);
        Ok(())
    }

    #[test]
    fn a_project_can_enable_waiting_wakes() -> Result<(), ConfigError> {
        let config = parse(&format!("{WAKE_PROJECT}orchestrator_waits = true\n"))?;
        assert!(config.projects.get("example.com").is_some_and(|p| p.orchestrator_waits));
        Ok(())
    }

    #[test]
    fn retired_config_keys_are_refused() {
        let base = "[mork]\nproject = \"f\"\n[git]\nauthor = \"A <a@b>\"\n";
        for seat in ["assistant", "opus", "astra", "fable", "mocker", "builder"] {
            let text = format!("{base}[seats.{seat}]\ncli = \"claude\"\n");
            assert!(matches!(parse(&text), Err(ConfigError::UnknownSeat { name, .. }) if name == seat));
        }
        let unknown = [
            format!("{base}[[picks.builder]]\ncli = \"claude\"\nmodel = \"m\"\nlabel = \"L\"\nefforts = [\"high\"]\n"),
            format!("{base}[seats.foreman.spec]\ncli = \"claude\"\n"),
            format!("{base}[seats.foreman.challenge]\ncli = \"claude\"\n"),
            format!("{base}[survey]\nper_day = 2\n"),
            format!("{WAKE_PROJECT}risk = [\"money\"]\n"),
            format!("{WAKE_PROJECT}window = 2\n"),
        ];
        for text in unknown {
            assert!(matches!(parse(&text), Err(ConfigError::Toml { .. })), "expected refusal for:\n{text}");
        }
    }

    #[test]
    fn context_cap_defaults_validates_bounds_and_allows_zero() -> Result<(), ConfigError> {
        let base = "[mork]\nproject = \"f\"\n";
        let tail = "[git]\nauthor = \"A <a@b>\"\n";
        assert_eq!(parse(&format!("{base}{tail}"))?.mork.context_cap(), Some(200_000));
        assert_eq!(parse(&format!("{base}context_cap = 0\n{tail}"))?.mork.context_cap(), None);
        assert_eq!(parse(&format!("{base}context_cap = 100000\n{tail}"))?.mork.context_cap(), Some(100_000));
        assert_eq!(parse(&format!("{base}context_cap = 1000000\n{tail}"))?.mork.context_cap(), Some(1_000_000));
        for bad in [1, 99_999, 1_000_001] {
            let text = format!("{base}context_cap = {bad}\n{tail}");
            let Err(ConfigError::Value { line, message }) = parse(&text) else {
                panic!("expected context_cap={bad} to fail");
            };
            assert_eq!(line, 3);
            assert!(message.contains("context_cap"), "{message}");
        }
        Ok(())
    }

    #[test]
    fn optional_mork_project_and_internals_hide_defaults() -> Result<(), ConfigError> {
        let no_project = "[mork]\n[git]\nauthor = \"A <a@b>\"\n";
        let cfg = parse(no_project)?;
        assert_eq!(cfg.mork.project, None);
        assert!(!cfg.internals.hide);

        let with_hide = "[mork]\n[git]\nauthor = \"A <a@b>\"\n[internals]\nhide = true\n";
        assert!(parse(with_hide)?.internals.hide);

        let scalar_mork = "mork = \"nope\"\n[git]\nauthor = \"A <a@b>\"\n";
        let Err(ConfigError::NotATable { line, name, .. }) = parse(scalar_mork) else {
            panic!("expected scalar mork to fail as NotATable");
        };
        assert_eq!(line, 1);
        assert_eq!(name, "mork");

        let missing_mork = "[git]\nauthor = \"A <a@b>\"\n";
        assert!(matches!(parse(missing_mork), Err(ConfigError::Toml { .. })));
        Ok(())
    }
}
