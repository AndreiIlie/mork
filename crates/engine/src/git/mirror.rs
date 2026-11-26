//! The mirrors (4.2.1): bare repositories the engine owns, never inside a wall.

use std::path::Path;
use std::time::Duration;

use mork_types::review::Contents;

use super::{GitError, git, git_env};
use crate::paths::Home;

const FETCH: Duration = Duration::from_secs(300);

/// A mirror's folder, made bare the first time.
pub fn ensure(home: &Home, project: &str) -> Result<std::path::PathBuf, GitError> {
    let m = home.mirrors().join(project);
    if !m.join("HEAD").exists() {
        std::fs::create_dir_all(&m).map_err(|source| GitError::Io { path: m.clone(), source })?;
        git(home, &m, &["init", "-q", "--bare"], FETCH)?;
    }
    Ok(m)
}

/// Fetches every branch of `from` (a URL, or the operator's checkout) into the mirror's own branches.
/// `env` carries the token, never the argv.
pub fn fetch(home: &Home, mirror: &Path, from: &str, env: &[(String, String)]) -> Result<(), GitError> {
    git_env(home, mirror, &["fetch", "-q", "--prune", "--no-tags", from, "+refs/heads/*:refs/heads/*"], FETCH, env)
        .map(|_| ())
}

/// A ref's commit, if it exists.
pub fn rev(home: &Home, mirror: &Path, r: &str) -> Option<String> {
    let spec = format!("{r}^{{commit}}");
    git(home, mirror, &["rev-parse", "--verify", "-q", &spec], FETCH).ok().filter(|s| s.len() == 40)
}

/// A branch of the mirror, as one read of them all lists it.
pub struct Head {
    /// Its name as `git` shortens it.
    pub short: String,
    /// Its name under `refs/heads/`.
    pub name: String,
    /// The commit it points at, as [`rev`] reads it.
    pub commit: Option<String>,
}

/// Every branch of the mirror, in `git`'s order, in one read; none when it can't be read.
pub fn heads(home: &Home, mirror: &Path) -> Vec<Head> {
    let format =
        "--format=%(refname)%00%(refname:short)%00%(objecttype)%00%(objectname)%00%(*objecttype)%00%(*objectname)";
    let Ok(out) = git(home, mirror, &["for-each-ref", format, "refs/heads/"], FETCH) else { return Vec::new() };
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\0').collect();
            let name = f.first()?.strip_prefix("refs/heads/")?.to_owned();
            // A branch at a tag counts as the commit the tag points at, as `^{commit}` peels it.
            let commit = match (f.get(2), f.get(4)) {
                (Some(&"commit"), _) => f.get(3),
                (_, Some(&"commit")) => f.get(5),
                _ => None,
            };
            let commit = commit.filter(|c| c.len() == 40).map(|c| (*c).to_owned());
            Some(Head { short: (*f.get(1)?).to_owned(), name, commit })
        })
        .collect()
}

/// Whether `a` is an ancestor of `b` (or the same).
pub fn is_ancestor(home: &Home, mirror: &Path, a: &str, b: &str) -> bool {
    git(home, mirror, &["merge-base", "--is-ancestor", a, b], FETCH).is_ok()
}

/// Where `head` left `base`.
pub fn merge_base(home: &Home, mirror: &Path, a: &str, b: &str) -> Result<String, GitError> {
    git(home, mirror, &["merge-base", a, b], FETCH)
}

/// The files `head` changes against `fork`, and how many lines.
pub fn changed(home: &Home, mirror: &Path, fork: &str, head: &str) -> Result<(Vec<String>, u64), GitError> {
    let numstat = git(home, mirror, &["diff", "--numstat", "--no-renames", fork, head], FETCH)?;
    let mut files = Vec::new();
    let mut lines = 0u64;
    for l in numstat.lines() {
        let mut parts = l.splitn(3, '\t');
        let (a, d, f) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"), parts.next().unwrap_or(""));
        lines += a.parse::<u64>().unwrap_or(0) + d.parse::<u64>().unwrap_or(0);
        if !f.is_empty() {
            files.push(f.to_owned());
        }
    }
    Ok((files, lines))
}

/// The paths `to` changes against `from`, as git names them, unquoted; a rename is a deletion and
/// an addition (fac-159).
pub fn paths_changed(home: &Home, mirror: &Path, from: &str, to: &str) -> Result<Vec<String>, GitError> {
    let out = git(home, mirror, &["diff", "--name-only", "-z", "--no-renames", from, to], FETCH)?;
    Ok(out.split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect())
}

/// What `head` makes of each file it changes against `fork`: its mode and blob, a deleted file's
/// zeros; a rename is a deletion and an addition (fac-95). Its paths are as git names them, unquoted.
pub fn contents(home: &Home, mirror: &Path, fork: &str, head: &str) -> Result<Contents, GitError> {
    let raw = git(home, mirror, &["diff", "--raw", "-z", "--no-renames", "--no-abbrev", fork, head], FETCH)?;
    let mut parts = raw.split('\0');
    let mut out = Contents::new();
    while let (Some(meta), Some(path)) = (parts.next(), parts.next()) {
        let f: Vec<&str> = meta.split_whitespace().collect();
        if let (Some(mode), Some(blob)) = (f.get(1), f.get(3)) {
            out.insert(path.to_owned(), format!("{mode} {blob}"));
        }
    }
    Ok(out)
}

/// What `commit` has of each of `paths`, as [`contents`] reads it: its mode and blob, a missing
/// one's zeros (fac-95).
pub fn contents_at(home: &Home, mirror: &Path, commit: &str, paths: &[&str]) -> Result<Contents, GitError> {
    let zeros = format!("000000 {}", "0".repeat(commit.len()));
    let mut out: Contents = paths.iter().map(|p| ((*p).to_owned(), zeros.clone())).collect();
    if paths.is_empty() {
        return Ok(out);
    }
    let mut args = vec!["--literal-pathspecs", "ls-tree", "-z", commit, "--"];
    args.extend_from_slice(paths);
    for entry in git(home, mirror, &args, FETCH)?.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let f: Vec<&str> = meta.split_whitespace().collect();
        if let (Some(mode), Some(blob), true) = (f.first(), f.get(2), out.contains_key(path)) {
            out.insert(path.to_owned(), format!("{mode} {blob}"));
        }
    }
    Ok(out)
}

/// Points `refs/mork/<name>` at a commit and writes it as a bundle: what a wall job takes
/// into a clone (4.2.2, 4.2.4). The ref's name is what the job fetches.
pub fn bundle_out(home: &Home, mirror: &Path, name: &str, commit: &str, out: &Path) -> Result<String, GitError> {
    let r = format!("refs/mork/{name}");
    git(home, mirror, &["update-ref", &r, commit], FETCH)?;
    let out_s = out.display().to_string();
    git(home, mirror, &["bundle", "create", "-q", &out_s, &r], FETCH)?;
    Ok(r)
}

/// Verifies a clone's bundle and fetches its one ref into `dest`; the commit it brought.
pub fn bundle_in(home: &Home, mirror: &Path, bundle: &Path, dest: &str) -> Result<String, GitError> {
    let b = bundle.display().to_string();
    git(home, mirror, &["bundle", "verify", "-q", &b], FETCH)?;
    let heads = git(home, mirror, &["bundle", "list-heads", &b], FETCH)?;
    let src = heads
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or_else(|| GitError::Refused("the bundle holds no ref".into()))?;
    let spec = format!("+{src}:{dest}");
    git(home, mirror, &["fetch", "-q", "--no-tags", &b, &spec], FETCH)?;
    rev(home, mirror, dest).ok_or_else(|| GitError::Refused(format!("{dest} isn't in the mirror after the fetch")))
}

/// Sets a ref (a card's head, kept after its clone goes).
pub fn set_ref(home: &Home, mirror: &Path, r: &str, commit: &str) -> Result<(), GitError> {
    git(home, mirror, &["update-ref", r, commit], FETCH).map(|_| ())
}

pub fn tree(home: &Home, mirror: &Path, commit: &str) -> Option<String> {
    let spec = format!("{commit}^{{tree}}");
    git(home, mirror, &["rev-parse", "--verify", "-q", &spec], FETCH).ok()
}

pub fn parents(home: &Home, mirror: &Path, commit: &str) -> Vec<String> {
    git(home, mirror, &["rev-list", "--parents", "-n", "1", commit], FETCH)
        .map(|o| o.split_whitespace().skip(1).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Pushes a commit to a branch of origin, never forced (4.10.1).
pub fn push(
    home: &Home,
    mirror: &Path,
    to: &str,
    commit: &str,
    branch: &str,
    env: &[(String, String)],
) -> Result<(), GitError> {
    let spec = format!("{commit}:refs/heads/{branch}");
    git_env(home, mirror, &["push", "-q", "--no-verify", to, &spec], FETCH, env).map(|_| ())
}

/// Where origin's branch is (`git ls-remote`).
pub fn ls_remote(
    home: &Home,
    mirror: &Path,
    from: &str,
    branch: &str,
    env: &[(String, String)],
) -> Result<Option<String>, GitError> {
    let r = format!("refs/heads/{branch}");
    let out = git_env(home, mirror, &["ls-remote", from, &r], FETCH, env)?;
    Ok(out.split_whitespace().next().map(str::to_owned))
}

/// The commits of `head` that none of `not` holds, newest first, each as its short hash and
/// subject; a ref the mirror lacks holds nothing, and a `head` it lacks has no commits (fac-171).
pub fn past(home: &Home, mirror: &Path, head: &str, not: &[String]) -> Result<Vec<String>, GitError> {
    if rev(home, mirror, head).is_none() {
        return Ok(Vec::new());
    }
    let mut args = vec!["log", "--format=%h %s", head, "--not"];
    args.extend(not.iter().filter(|r| rev(home, mirror, r).is_some()).map(String::as_str));
    Ok(git(home, mirror, &args, FETCH)?.lines().map(str::to_owned).collect())
}

/// The messages of the commits a push adds, for the leak and internals checks (3.8, 3.10.6): `to`'s
/// commits reachable from neither `from` nor `base`, so the base's merged-in history isn't checked.
pub fn messages(home: &Home, mirror: &Path, from: &str, to: &str, base: &str) -> Result<String, GitError> {
    git(home, mirror, &["log", "--format=%B", to, "--not", from, base], FETCH)
}

/// The kind and location of a known secret added by any commit in the range or its final diff.
pub fn added_secret(
    home: &Home,
    mirror: &Path,
    from: &str,
    to: &str,
    secrets: &crate::secrets::Secrets,
) -> Result<Option<String>, String> {
    let range = format!("{from}..{to}");
    let flags = ["--no-color", "--no-ext-diff", "--no-textconv", "--text", "--unified=0"];
    // A merge commit is diffed against each parent so additions in its resolution are checked.
    let mut log = vec!["log", "-p", "--diff-merges=separate", "--format=commit %H"];
    log.extend(flags);
    log.push(&range);
    let mut diff = vec!["diff"];
    diff.extend(flags);
    diff.extend([from, to]);
    for args in [log, diff] {
        let patch = git(home, mirror, &args, FETCH).map_err(|e| secrets.mask(&e.to_string()))?;
        if let Some(hit) = patch_secret(&patch, to, secrets) {
            return Ok(Some(hit));
        }
    }
    Ok(None)
}

// Within a hunk, `+++ x` adds the text `++ x`; only outside a hunk does it name a file.
fn patch_secret<'a>(patch: &'a str, mut commit: &'a str, secrets: &crate::secrets::Secrets) -> Option<String> {
    let mut file = "";
    let mut n = 0u64;
    let (mut old, mut new) = (0u64, 0u64);
    for l in patch.lines() {
        if old > 0 || new > 0 {
            match l.as_bytes().first() {
                Some(b'+') => {
                    if let Some(kind) = secrets.hit(&l[1..]) {
                        return Some(secrets.mask(&format!("{kind}, in {file}:{n} (commit {commit})")));
                    }
                    n += 1;
                    new = new.saturating_sub(1);
                }
                Some(b'-') => old = old.saturating_sub(1),
                Some(b'\\') => {}
                _ => {
                    n += 1;
                    old = old.saturating_sub(1);
                    new = new.saturating_sub(1);
                }
            }
        } else if let Some(at) = l.strip_prefix("commit ") {
            commit = at;
        } else if let Some(f) = l.strip_prefix("+++ ") {
            file = f.strip_prefix("b/").unwrap_or(f).trim_end_matches('\t');
        } else if let Some(h) = l.strip_prefix("@@ ") {
            // `@@ -a,b +c,d @@`: a missing count is 1.
            let side = |sign: char| -> (u64, u64) {
                let p = h.split_whitespace().find_map(|p| p.strip_prefix(sign)).unwrap_or("1");
                let mut it = p.split(',');
                let start = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
                (start, it.next().and_then(|x| x.parse().ok()).unwrap_or(1))
            };
            (n, new) = side('+');
            old = side('-').1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    // why: a failed fixture setup must fail the test.
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::secrets::Secrets;

    #[test]
    fn removed_credential_is_located_in_history_without_quoting_it() {
        let dir = std::env::temp_dir().join(format!("mork-test-secret-diff-{}", crate::random::salt()));
        std::fs::create_dir(&dir).unwrap();
        let home = Home::new(dir.join("home"));
        let run = |args: &[&str]| git(&home, &dir, args, FETCH).unwrap();
        run(&["init", "-q"]);
        run(&["config", "user.name", "Example"]);
        run(&["config", "user.email", "example@example.invalid"]);
        run(&["commit", "--allow-empty", "-qm", "base"]);
        let base = run(&["rev-parse", "HEAD"]);
        let token = "synthetic-credential";
        let mut secrets = Secrets::default();
        secrets.add("a credential", token);
        std::fs::write(dir.join("fixture"), format!("first line\n++ {token}\n")).unwrap();
        run(&["add", "fixture"]);
        run(&["commit", "-qm", "add fixture"]);
        let added = run(&["rev-parse", "HEAD"]);
        run(&["rm", "fixture"]);
        run(&["commit", "-qm", "delete fixture"]);
        let head = run(&["rev-parse", "HEAD"]);
        assert_eq!(added_secret(&home, &dir, &added, &head, &secrets).unwrap(), None);
        let hit = added_secret(&home, &dir, &base, &head, &secrets).unwrap();
        assert_eq!(hit, Some(format!("a credential, in fixture:2 (commit {added})")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
