//! The synchronous PreToolUse decisions passed to Claude on its command line.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const INVALID: &str = "Cannot verify the Claude tool request.";
const PROTECTED: &str = "The request could access ACCESS.md or files under .ssh.";

type Input = Map<String, Value>;

/// The Agent restriction applies on every Claude seat. The path restriction belongs to unwalled seats.
pub enum Hook {
    Agent { fable: bool },
    Path,
}

impl Hook {
    /// An error refuses the tool call, including when its input cannot be checked.
    pub fn check(self, reader: impl Read) -> Result<(), &'static str> {
        let event: Value = serde_json::from_reader(reader).map_err(|_| INVALID)?;
        let name = event["tool_name"].as_str().ok_or(INVALID)?;
        let input = event["tool_input"].as_object().ok_or(INVALID)?;
        match self {
            Self::Agent { fable } if name == "Agent" => {
                let model = optional(input, "model")?;
                let kind = optional(input, "subagent_type")?;
                match mork_core::agents::subagents::refusal(model, kind, fable) {
                    Some(reason) => Err(reason),
                    None => Ok(()),
                }
            }
            Self::Agent { .. } => Err(INVALID),
            Self::Path => check_path(name, input, event["cwd"].as_str()),
        }
    }
}

fn optional<'a>(input: &'a Input, key: &str) -> Result<Option<&'a str>, &'static str> {
    input.get(key).map(|v| v.as_str().filter(|s| !s.is_empty()).ok_or(INVALID)).transpose()
}

fn required<'a>(input: &'a Input, key: &str) -> Result<&'a str, &'static str> {
    optional(input, key)?.ok_or(INVALID)
}

fn check_path(name: &str, input: &Input, cwd: Option<&str>) -> Result<(), &'static str> {
    let safe = match name {
        "Bash" => {
            let command = required(input, "command")?;
            !command.contains("ACCESS.md") && !command.contains(".ssh")
        }
        "Read" => checked_path(required(input, "file_path")?, cwd)?.is_file(),
        "Glob" | "Grep" => {
            let pattern = required(input, "pattern")?;
            let root = checked_path(optional(input, "path")?.unwrap_or("."), cwd)?;
            if name == "Glob" { safe_glob(&root, pattern) } else { root.is_file() }
        }
        _ => return Err(INVALID),
    };
    if safe { Ok(()) } else { Err(PROTECTED) }
}

fn protected(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "ACCESS.md") || path.iter().any(|part| part == ".ssh")
}

fn checked_path(value: &str, cwd: Option<&str>) -> Result<PathBuf, &'static str> {
    let path = Path::new(value);
    let path = if path.starts_with("~") {
        let home = std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).ok_or(INVALID)?;
        home.join(path.strip_prefix("~").map_err(|_| INVALID)?)
    } else if path.is_relative() {
        let base = cwd.map(Path::new).filter(|p| p.is_absolute()).ok_or(INVALID)?;
        base.join(path)
    } else {
        path.to_owned()
    };
    if protected(&path) {
        return Err(PROTECTED);
    }
    let resolved = path.canonicalize().map_err(|_| PROTECTED)?;
    if protected(&resolved) { Err(PROTECTED) } else { Ok(resolved) }
}

// A fixed directory and a single filename pattern bound the search before traversal. Other scopes are uncertain.
fn safe_glob(root: &Path, pattern: &str) -> bool {
    // Claude passes basename-only globs to a recursive file search.
    let Some((parent, leaf)) = pattern.rsplit_once('/') else { return false };
    let literal = |b: u8| b.is_ascii_alphanumeric() || b"._- ".contains(&b);
    if !parent.split('/').all(|part| !matches!(part, "" | "." | "..") && part.bytes().all(literal)) {
        return false;
    }
    let directory = root.join(parent);
    if protected(&directory) || !directory.canonicalize().is_ok_and(|p| p.is_dir() && !protected(&p)) {
        return false;
    }
    if leaf.is_empty() || !leaf.bytes().all(|b| literal(b) || b == b'*') {
        return false;
    }
    if let Some((prefix, suffix)) = leaf.split_once('*') {
        return !suffix.contains('*')
            && !["ACCESS.md", ".ssh"].iter().any(|name| name.starts_with(prefix) && name.ends_with(suffix));
    }
    let target = directory.join(leaf);
    !matches!(leaf, "." | "..")
        && !protected(&target)
        && target.canonicalize().is_ok_and(|p| p.is_file() && !protected(&p))
}

#[cfg(test)]
mod tests {
    // why: a fixture setup failure fails the test.
    #![allow(clippy::unwrap_used)]

    use super::Hook;
    use serde_json::{Value, json};

    #[test]
    fn malformed_requests_and_disallowed_models_are_refused() {
        for hook in [Hook::Path, Hook::Agent { fable: false }] {
            assert!(hook.check(&b"{"[..]).is_err());
        }
        let agent = |input: Value, fable| {
            let event = json!({"tool_name":"Agent", "tool_input":input});
            Hook::Agent { fable }.check(event.to_string().as_bytes())
        };
        assert!(agent(Value::Null, false).is_err());
        assert!(agent(json!({"model":1}), false).is_err());
        assert!(agent(json!({"model":"fable"}), false).is_err());
        assert!(agent(json!({}), true).is_err());
        assert!(agent(json!({"model":"sonnet"}), true).is_ok());
    }

    #[test]
    fn unwalled_tools_require_a_scope_that_excludes_protected_paths() {
        let root = std::env::temp_dir().join(format!("mork-hooks-{}", crate::random::uuid()));
        let ssh = root.join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        for path in [root.join("ACCESS.md"), root.join("notes.txt"), ssh.join("config")] {
            std::fs::write(path, "").unwrap();
        }
        std::os::unix::fs::symlink(&ssh, root.join("linked")).unwrap();
        let check = |name, input: Value| {
            let event = json!({"tool_name":name, "cwd":root, "tool_input":input});
            Hook::Path.check(event.to_string().as_bytes())
        };
        for target in ["ACCESS.md", ".ssh/config", "linked/config"] {
            assert!(check("Read", json!({"file_path":target})).is_err());
            assert!(check("Grep", json!({"path":target, "pattern":"text"})).is_err());
        }
        assert!(check("Read", json!({"file_path":"notes.txt"})).is_ok());
        assert!(check("Grep", json!({"path":"notes.txt", "pattern":"text"})).is_ok());
        assert!(check("Grep", json!({"pattern":"text"})).is_err());
        assert!(check("Glob", json!({"pattern":"src/*.txt"})).is_ok());
        for pattern in ["*", "*.txt", "**/*.txt", "src/*.md", "linked/*", ".ssh/config"] {
            assert!(check("Glob", json!({"pattern":pattern})).is_err());
        }
        assert!(check("Glob", json!({"path":123, "pattern":"*.txt"})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bash_retains_the_literal_command_denials() {
        let check = |input: Value| {
            let event = json!({"tool_name":"Bash", "tool_input":input});
            Hook::Path.check(event.to_string().as_bytes())
        };
        assert!(check(json!({})).is_err());
        assert!(check(json!({"command":"cat ACCESS.md"})).is_err());
        assert!(check(json!({"command":"ls .ssh"})).is_err());
        assert!(check(json!({"command":"pwd"})).is_ok());
    }
}
