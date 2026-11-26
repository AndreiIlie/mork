//! A local `/merge` in the operator's checkout (4.11.3): checked before anything moves, with an optional
//! non-forced backup push after landing.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use mork_core::queue::checkout::{Facts, Move};

use super::{GitError, git, git_env, mirror};
use crate::paths::Home;

const T: Duration = Duration::from_secs(120);

/// Where the checkout's base is.
pub fn base_at(home: &Home, checkout: &Path, base: &str) -> Option<String> {
    let r = format!("refs/heads/{base}^{{commit}}");
    git(home, checkout, &["rev-parse", "--verify", "-q", &r], T).ok().filter(|s| s.len() == 40)
}

fn lines(s: &str) -> Vec<String> {
    s.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

// Read right before a move, so `core` decides from what is there then (4.11.3).
fn facts(home: &Home, checkout: &Path, base: &str, cur: &str, target: &str) -> Result<Facts, GitError> {
    let ancestor = |a: &str, b: &str| git(home, checkout, &["merge-base", "--is-ancestor", a, b], T).is_ok();
    Ok(Facts {
        on: git(home, checkout, &["symbolic-ref", "-q", "--short", "HEAD"], T).unwrap_or_default(),
        base: base.to_owned(),
        cur: cur.to_owned(),
        target: target.to_owned(),
        target_in_cur: ancestor(target, cur),
        cur_in_target: ancestor(cur, target),
        touched: lines(&git(home, checkout, &["diff", "--name-only", cur, target], T)?),
        changed: lines(&git(home, checkout, &["diff", "--name-only", "HEAD"], T)?),
        loose: lines(&git(home, checkout, &["ls-files", "--others", "--exclude-standard"], T)?),
        ignored: lines(&git(home, checkout, &["ls-files", "--others", "--ignored", "--exclude-standard"], T)?),
    })
}

/// Sets the mirror's base to `target`, where the checkout's base already is, so a take forks from the
/// landed tip (fac-613). A refusal says the checkout moved and retrying finishes the mirror.
pub fn follow(home: &Home, mirror_dir: &Path, base: &str, target: &str) -> Result<(), GitError> {
    mirror::set_ref(home, mirror_dir, &format!("refs/heads/{base}"), target).map_err(|e| {
        GitError::Refused(format!(
            "{base} in the operator's checkout already moved to {target}, but the mirror's {base} couldn't follow: {e}. \
             Retry finishes it"
        ))
    })
}

/// Moves the checkout's base, then the mirror's, to `target`; or reports `base_moved` with the tip when
/// a join is needed first. A mirror that can't follow fails with the checkout moved: a retry sets it alone.
pub fn move_base(
    home: &Home,
    checkout: &Path,
    mirror_dir: &Path,
    card: &str,
    base: &str,
    target: &str,
) -> Result<Value, GitError> {
    let cur = base_at(home, checkout, base)
        .ok_or_else(|| GitError::Refused(format!("The operator's checkout has no {base}")))?;
    if cur != target {
        let r = format!("refs/mork/land-{card}");
        mirror::set_ref(home, mirror_dir, &r, target)?;
        let from = mirror_dir.display().to_string();
        git(home, checkout, &["fetch", "-q", "--no-tags", &from, &r], T)?;
    }
    match mork_core::queue::checkout::decide(&facts(home, checkout, base, &cur, target)?) {
        Move::Done => {
            follow(home, mirror_dir, base, target)?;
            Ok(json!({"base_moved": false, "before": cur, "after": target}))
        }
        Move::Refused(why) => Err(GitError::Refused(why)),
        Move::BaseMoved => {
            mirror::fetch(home, mirror_dir, &checkout.display().to_string(), &[])?;
            Ok(json!({"base_moved": true, "base_tip": cur}))
        }
        Move::FastForward => {
            git(home, checkout, &["merge", "-q", "--ff-only", target], T)?;
            let after = base_at(home, checkout, base).unwrap_or_default();
            let head = git(home, checkout, &["rev-parse", "HEAD"], T).unwrap_or_default();
            if after != target || head != target {
                return Err(GitError::Refused(format!(
                    "after the merge {base} is at {after} and HEAD at {head}, not at {target}"
                )));
            }
            follow(home, mirror_dir, base, target)?;
            Ok(json!({"base_moved": false, "before": cur, "after": target}))
        }
    }
}

/// Pushes the current base to its configured backup remote, without forcing or asking for input.
pub fn push_backup(home: &Home, checkout: &Path, base: &str, remote: &str) -> Result<Value, GitError> {
    let branch = format!("refs/heads/{base}:refs/heads/{base}");
    let env = [("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes".into())];
    git_env(home, checkout, &["push", "--", remote, &branch], T, &env)?;
    Ok(json!({}))
}

/// After a crash (2.5.2): the base at the target is done; at `from`, the move runs; anywhere
/// else, it is held.
pub fn found(home: &Home, checkout: &Path, base: &str, from: &str, target: &str) -> Result<Option<Value>, GitError> {
    let cur = base_at(home, checkout, base)
        .ok_or_else(|| GitError::Refused(format!("The operator's checkout has no {base}")))?;
    if cur == target {
        Ok(Some(json!({"base_moved": false, "before": from, "after": target})))
    } else if cur == from {
        Ok(None)
    } else {
        Err(GitError::Refused(format!("{base} is at {cur}: neither where the merge started nor its target")))
    }
}
