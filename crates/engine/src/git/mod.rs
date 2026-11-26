//! The engine's git (1.12.2): on the operator's checkouts and the engine's own repositories, never
//! on an agent's clone (1.2.4), each command a short one through `jobs`.

pub mod checkout;
pub mod mirror;

use std::path::Path;
use std::time::Duration;

use crate::jobs::launch;
use crate::paths::Home;

const SHORT: Duration = Duration::from_secs(20);

/// What went wrong with a git command, or with what the engine checked around it.
#[derive(Debug)]
pub enum GitError {
    /// git couldn't run, or didn't finish in time.
    Run { args: String, source: std::io::Error },
    /// git ran and refused: its own words.
    Failed { args: String, stderr: String },
    /// A file around the command (a mirror's folder).
    Io { path: std::path::PathBuf, source: std::io::Error },
    /// The engine's own check refused, before or after git: why.
    Refused(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run { args, source } => write!(f, "git {args}: {source}"),
            Self::Failed { args, stderr } if stderr.is_empty() => {
                write!(f, "git {} failed", args.split(' ').next().unwrap_or_default())
            }
            Self::Failed { stderr, .. } => write!(f, "{stderr}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<GitError> for String {
    fn from(e: GitError) -> Self {
        e.to_string()
    }
}

/// Runs git in a folder (a mirror or the operator's checkout); its trimmed output, or git's words.
pub fn git(home: &Home, dir: &Path, args: &[&str], timeout: Duration) -> Result<String, GitError> {
    git_env(home, dir, args, timeout, &[])
}

/// As [`git`], with variables (a token's header) that never reach the argv.
pub fn git_env(
    home: &Home,
    dir: &Path,
    args: &[&str],
    timeout: Duration,
    env: &[(String, String)],
) -> Result<String, GitError> {
    let mut argv = vec!["git"];
    argv.extend_from_slice(args);
    let mut all: Vec<(String, String)> = vec![("GIT_TERMINAL_PROMPT".into(), "0".into())];
    all.extend_from_slice(env);
    let out = launch::short_env(home.root(), &argv, dir, timeout, &all)
        .map_err(|source| GitError::Run { args: args.join(" "), source })?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(GitError::Failed { args: args.join(" "), stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned() })
    }
}

/// The environment that gives git a token's header for one command (`GIT_CONFIG_*`), so the
/// token is never in an argv or a file.
pub fn token_env(header: &str) -> Vec<(String, String)> {
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
        ("GIT_CONFIG_VALUE_0".into(), format!("Authorization: {header}")),
    ]
}

/// Whether `commit` is on `base`, read without fetching (1.8.5); `None` when git can't say. `base`
/// is the branch, never a tag of the same name (M5.10).
pub fn on_master(home: &Home, repo: &Path, base: &str, commit: &str) -> Option<bool> {
    let branch = if base.starts_with("refs/") { base.to_owned() } else { format!("refs/heads/{base}") };
    let out = launch::short(home.root(), &["git", "merge-base", "--is-ancestor", commit, &branch], repo, SHORT).ok()?;
    match out.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// Whether `older` is `newer` or one of its ancestors, read without fetching. `None` when git
/// can't say.
pub fn descends(home: &Home, repo: &Path, older: &str, newer: &str) -> Option<bool> {
    let out = launch::short(home.root(), &["git", "merge-base", "--is-ancestor", older, newer], repo, SHORT).ok()?;
    match out.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// Resolves a commit to its full hash.
pub fn resolve(home: &Home, repo: &Path, commit: &str) -> Result<String, GitError> {
    let spec = format!("{commit}^{{commit}}");
    let out = launch::short(home.root(), &["git", "rev-parse", "--verify", "--quiet", &spec], repo, SHORT)
        .map_err(|source| GitError::Run { args: format!("rev-parse {spec}"), source })?;
    let full = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() && full.len() == 40 {
        Ok(full)
    } else {
        Err(GitError::Refused(format!("{commit} isn't a commit of Mork repo")))
    }
}

pub fn archive(home: &Home, repo: &Path, commit: &str, tar: &Path) -> Result<(), GitError> {
    let tar_s = tar.display().to_string();
    let out = launch::short(
        home.root(),
        &["git", "archive", "--format=tar", "-o", &tar_s, commit],
        repo,
        Duration::from_secs(120),
    )
    .map_err(|source| GitError::Run { args: format!("archive {commit}"), source })?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = format!("git archive: {}", String::from_utf8_lossy(&out.stderr).trim());
        Err(GitError::Failed { args: format!("archive {commit}"), stderr })
    }
}

/// A checkout's HEAD commit, read from its `.git` files with no git process (a folder read
/// every two seconds): `None` when it can't be read so.
pub fn head_of(dir: &Path) -> Option<String> {
    let git = dir.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(r) = head.strip_prefix("ref: ") else { return Some(head.to_owned()).filter(|h| h.len() == 40) };
    if let Ok(c) = std::fs::read_to_string(git.join(r)) {
        return Some(c.trim().to_owned()).filter(|h| h.len() == 40);
    }
    let packed = std::fs::read_to_string(git.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| {
        let (sha, name) = l.split_once(' ')?;
        (name == r && sha.len() == 40).then(|| sha.to_owned())
    })
}
