// The actions that reach Bitbucket (4.10, 4.11, 4.16): each is checked for leaks (3.8) and for
// the factory's internals (3.10.6) before anything leaves.

use factory_types::action::ActionWhat;
use factory_types::config::ProjectConfig as Project;
use factory_types::work::{ActionFailure, PrState};
use factory_types::{CardId, Sha};
use serde_json::{Value, json};

use super::act_run::{Call, failed, git_failure, source};
use crate::bitbucket::{calls, client};
use crate::git::mirror;
use crate::secrets;

pub fn repo(p: &Project) -> Result<client::Repo, ActionFailure> {
    let token = crate::token::read().map_err(|e| ActionFailure::Token { words: e.to_string() })?;
    repo_as(p, token.header())
}

fn repo_as(p: &Project, header: String) -> Result<client::Repo, ActionFailure> {
    let bb = p.bitbucket.as_ref().ok_or_else(|| failed("the project isn't on Bitbucket"))?;
    Ok(client::Repo { api: client::api(), workspace: bb.workspace.clone(), slug: bb.slug.clone(), header })
}

// Every PR opens in Andrei's name with his own token (fac-98, fac-111); a card's own PR (`pr`), still
// open from its branch into the base, is retitled, and one changed or gone is held.
fn open_as_andrei(p: &Project, branch: &str, text: (&str, &str), pr: Option<u64>) -> Result<Value, ActionFailure> {
    let his = |e: String| failed(format!("Andrei's merge token: {e}"));
    let bot = repo(p)?;
    let open = match pr {
        Some(n) => {
            let seen = calls::pr_of(&bot, n)?;
            if !calls::is_mine(&seen, branch, &p.base) {
                return Err(failed(format!("PR {n} is no longer open from {branch} into {}", p.base)));
            }
            Some(seen)
        }
        None => calls::open_of_branch(&bot, branch, &p.base)?,
    };
    let token = crate::token::read_merge().map_err(|e| his(e.to_string()))?;
    let mine = repo_as(p, token.header())?;
    match open {
        Some(seen) => calls::retitle(&mine, &seen, text),
        None => calls::open_pr(&mine, branch, &p.base, text),
    }
    .map_err(|f| match f {
        ActionFailure::Token { words } => his(words),
        f => f,
    })
}

// A PR merges with Andrei's own token, as it opened (fac-98): its refusal is his token's, a refusal
// for conflicts (F-B7) told with the destination's tip and the files both sides change.
fn merge_as_andrei(
    c: &Call,
    p: &Project,
    project: &str,
    pr: u64,
    into: &str,
    head: &Sha,
) -> Result<Value, ActionFailure> {
    let his = |e: String| failed(format!("Andrei's merge token: {e}"));
    let token = crate::token::read_merge().map_err(|e| his(e.to_string()))?;
    match calls::merge(&repo_as(p, token.header())?, pr, false) {
        Ok(hash) => Ok(verify(c, p, project, &hash, head)),
        Err(ActionFailure::Token { words }) => Err(his(words)),
        Err(ActionFailure::Failed { words })
            if words.starts_with("Bitbucket answered 400:") && words.contains("merge conflicts") =>
        {
            let (tip, files) = conflict(c, p, project, into, head);
            Err(ActionFailure::Conflict { words, tip, files })
        }
        Err(f) => Err(f),
    }
}

// The destination's tip after a fetch, and the files the PR and it both change since they forked;
// what the mirror can't tell is left out.
fn conflict(c: &Call, p: &Project, project: &str, into: &str, head: &Sha) -> (Option<Sha>, Vec<String>) {
    let Ok(m) = c.mirror(project) else { return (None, Vec::new()) };
    if let Ok((from, env)) = source(p) {
        let _ = mirror::fetch(&c.home, &m, &from, &env);
    }
    let Some(tip) = mirror::rev(&c.home, &m, &format!("refs/heads/{into}")) else { return (None, Vec::new()) };
    let both = mirror::rev(&c.home, &m, head.as_str())
        .and_then(|h| Some((mirror::merge_base(&c.home, &m, &tip, &h).ok()?, h)))
        .and_then(|(fork, h)| {
            let ours = mirror::contents(&c.home, &m, &fork, &h).ok()?;
            let theirs = mirror::contents(&c.home, &m, &fork, &tip).ok()?;
            Some(ours.into_keys().filter(|f| theirs.contains_key(f)).collect())
        });
    (Some(Sha::new(tip)), both.unwrap_or_default())
}

pub fn approver() -> Option<String> {
    crate::token::read().ok().and_then(|t| t.approver)
}

fn check(c: &Call, text: &str, what: &str) -> Result<(), ActionFailure> {
    check_words(&c.home, &c.config, &c.secrets(), text, what)
}

pub(super) fn check_words(
    home: &crate::paths::Home,
    config: &factory_types::config::Config,
    secrets: &secrets::Secrets,
    text: &str,
    what: &str,
) -> Result<(), ActionFailure> {
    if let Some(kind) = secrets.hit(text) {
        return Err(ActionFailure::Leak { words: format!("leak: this would publish {kind}, in {what}") });
    }
    let home = home.root().display().to_string();
    if let Some(found) = secrets::internals(&config.internals.words, &home, text) {
        return Err(ActionFailure::Internals { words: format!("internals: {found} in {what}") });
    }
    Ok(())
}

pub fn run(c: &Call) -> Result<Value, ActionFailure> {
    match &c.action.what {
        ActionWhat::Push { card, branch, commit } => push(c, card, branch, commit),
        ActionWhat::OpenPr { project, branch, title, description, pr, .. } => {
            let p = c.project(project)?;
            check(c, title, "the PR's title")?;
            check(c, description, "the PR's description")?;
            open_as_andrei(&p, branch, (title, description), *pr)
        }
        ActionWhat::Comment { pr, file, line, body, .. } => {
            let k = c.card()?;
            let at = if file.is_empty() {
                "the comment on the PR".to_owned()
            } else {
                format!("the comment at {file}:{line}")
            };
            check(c, body, &at)?;
            calls::comment(&repo(&c.project(&k.project)?)?, *pr, file, *line, body)
        }
        ActionWhat::ReplyComment { pr, comment, body, .. } => {
            let k = c.card()?;
            check(c, body, "the reply on the PR")?;
            calls::reply(&repo(&c.project(&k.project)?)?, *pr, *comment, body)
        }
        // Nothing of ours is sent: the comment's id alone, its thread resolved as the review bot posted it.
        ActionWhat::ResolveComment { pr, comment, .. } => {
            let k = c.card()?;
            calls::resolve(&repo(&c.project(&k.project)?)?, *pr, *comment)
        }
        ActionWhat::Merge { pr, head, base, dest, .. } => merge(c, *pr, (head, base), dest.as_deref()),
        ActionWhat::Fetch { .. }
        | ActionWhat::Take { .. }
        | ActionWhat::TakeJoin { .. }
        | ActionWhat::BackupPush { .. }
        | ActionWhat::MoveBase { .. }
        | ActionWhat::CreateBoardCard { .. }
        | ActionWhat::ChangeBoardCard { .. }
        | ActionWhat::RemoveClone { .. }
        | ActionWhat::RemoveTrain { .. }
        | ActionWhat::Door(_) => Err(failed("not a Bitbucket action")),
    }
}

// Never forced, and only after the checks of what it adds.
fn push(c: &Call, card: &CardId, branch: &str, commit: &Sha) -> Result<Value, ActionFailure> {
    let k = c.card()?;
    let p = c.project(&k.project)?;
    let m = c.mirror(&k.project)?;
    let from = k
        .pushed
        .clone()
        .map(|s| s.0)
        .or_else(|| mirror::rev(&c.home, &m, &format!("refs/heads/{}", p.base)))
        .ok_or_else(|| failed("no base to push from"))?;
    let secrets = c.secrets();
    for (file, n, line) in mirror::added_lines(&c.home, &m, &from, commit.as_str()).map_err(failed)? {
        if let Some(kind) = secrets.hit(&line) {
            return Err(ActionFailure::Leak { words: format!("leak: this push would publish {kind}, in {file}:{n}") });
        }
    }
    let base = format!("refs/heads/{}", p.base);
    let messages = mirror::messages(&c.home, &m, &from, commit.as_str(), &base).map_err(failed)?;
    check(c, &messages, "a commit message")?;
    check(c, branch, "the branch's name")?;
    let (to, env) = source(&p).map_err(failed)?;
    match mirror::push(&c.home, &m, &to, commit.as_str(), branch, &env).map_err(String::from) {
        Ok(()) => Ok(json!({"pushed": commit, "card": card})),
        Err(words)
            // Only git's own words for a branch that moved: any other refusal (a hook's) is held
            // with its words (R4).
            if words.contains("non-fast-forward") || words.contains("fetch first") =>
        {
            let at = mirror::ls_remote(&c.home, &m, &to, branch, &env).ok().flatten().map(Sha::new);
            Err(ActionFailure::NotFastForward { at })
        }
        Err(words) => Err(git_failure(words)),
    }
}

// Every merge, retried too, needs Andrei's approval on the head acted and `into` where its run merged
// it in (fac-98, fac-111); an older build's merge with no destination goes into the base.
fn merge(c: &Call, pr: u64, (head, base): (&Sha, &Sha), dest: Option<&str>) -> Result<Value, ActionFailure> {
    let k = c.card()?;
    let p = c.project(&k.project)?;
    let repo = repo(&p)?;
    let into = dest.unwrap_or(&p.base);
    let approver = approver().unwrap_or_default();
    let (seen, _) = calls::read_pr(&repo, pr, &approver, &c.fulls)?;
    let why = match seen.state {
        PrState::Open if seen.head != *head => {
            Some(format!("PR {pr}'s head is {}, not the approved {}", seen.head.short(), head.short()))
        }
        PrState::Open if seen.dest.as_deref() != Some(into) => {
            Some(format!("PR {pr} merges into {}, not {into}", seen.dest.clone().unwrap_or_default()))
        }
        PrState::Open if approver.is_empty() => Some("the token file names no approver".to_owned()),
        PrState::Open if !seen.approved => {
            Some(format!("Andrei's approval doesn't stand on PR {pr}'s head {}", seen.head.short()))
        }
        PrState::Open => None,
        other => Some(format!("PR {pr} is {other:?}, not open")),
    };
    if let Some(words) = why {
        return Err(ActionFailure::Guard { words, pr: Some(Box::new(seen)) });
    }
    let m = c.mirror(&k.project)?;
    let (from, env) = source(&p).map_err(failed)?;
    mirror::fetch(&c.home, &m, &from, &env).map_err(failed)?;
    let tip = mirror::rev(&c.home, &m, &format!("refs/heads/{into}"))
        .ok_or_else(|| failed(format!("{} has no branch {into}", k.project)))?;
    if tip != base.0 {
        let tip = Sha::new(tip);
        return Err(ActionFailure::Moved { words: format!("{into} moved to {} since its review", tip.short()), tip });
    }
    merge_as_andrei(c, &p, &k.project, pr, into, head)
}

/// Whether a merge commit is what was tested.
pub fn verify(c: &Call, p: &Project, project: &str, hash: &str, head: &Sha) -> Value {
    let Ok(m) = c.mirror(project) else { return json!({"matches": false, "why": "no mirror"}) };
    if let Ok((from, env)) = source(p) {
        let _ = mirror::fetch(&c.home, &m, &from, &env);
    }
    let Some(commit) = mirror::rev(&c.home, &m, hash) else {
        return json!({"unreadable": format!("the merge commit {hash} isn't in the mirror after a fetch")});
    };
    // What the mirror says of it; `core` decides whether it is what was tested (4.11.2.5, R21).
    json!({
        "commit": commit,
        "parents": mirror::parents(&c.home, &m, &commit),
        "tree": mirror::tree(&c.home, &m, &commit),
        "head_tree": mirror::tree(&c.home, &m, head.as_str()),
    })
}
