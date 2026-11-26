// The actions that reach Bitbucket (4.10, 4.11, 4.16): each is checked for leaks (3.8) and for
// Mork's internals (3.10.6) before anything leaves.

use mork_types::action::ActionWhat;
use mork_types::config::ProjectConfig as Project;
use mork_types::work::{ActionFailure, PrState};
use mork_types::{CardId, Sha};
use serde_json::{Value, json};

use super::act_run::{Call, failed, git_failure, source};
use crate::forge;
use crate::git::mirror;
use crate::secrets;

// A PR merges with the operator's own token, as it opened (fac-98): its refusal is his token's, a refusal
// for conflicts (F-B7) told with the destination's tip and the files both sides change.
fn merge_as_operator(
    c: &Call,
    p: &Project,
    project: &str,
    pr: u64,
    into: &str,
    head: &Sha,
) -> Result<Value, ActionFailure> {
    match forge::write(&p.forge)?.merge(pr, head.as_str()) {
        Ok(hash) => Ok(verify(c, p, project, &hash, head)),
        Err(ActionFailure::Conflict { words, .. }) => {
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

pub(super) fn landing(comment: &forge::Comment, file: &str, line: u64) -> Value {
    let anchor = comment.inline.as_ref();
    let path = anchor.and_then(|a| a.path.as_deref()).unwrap_or_default();
    let to = anchor.and_then(|a| a.line).unwrap_or_default();
    json!({"id": comment.id, "asked": format!("{file}:{line}"), "landed": format!("{path}:{to}")})
}

fn check(c: &Call, text: &str, what: &str) -> Result<(), ActionFailure> {
    check_words(&c.home, &c.config, &c.secrets(), text, what)
}

pub(super) fn check_words(
    home: &crate::paths::Home,
    config: &mork_types::config::Config,
    secrets: &secrets::Secrets,
    text: &str,
    what: &str,
) -> Result<(), ActionFailure> {
    if let Some(kind) = secrets.hit(text) {
        return Err(ActionFailure::Leak { words: format!("leak: this would publish {kind}, in {what}") });
    }
    let home = home.root().display().to_string();
    if let Some(found) = secrets::internals(&config.internals, &home, text) {
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
            Ok(json!({"pr": forge::write(&p.forge)?.publish(branch, &p.base, (title, description), *pr)?}))
        }
        ActionWhat::Comment { pr, file, line, body, source, .. } => {
            let k = c.card()?;
            let at = if file.is_empty() {
                "the comment on the PR".to_owned()
            } else {
                format!("the comment at {file}:{line}")
            };
            check(c, body, &at)?;
            let cfg = c.project(&k.project)?.forge;
            let source = source.as_ref();
            let inline = matches!(cfg, mork_types::config::ForgeConfig::Github { .. })
                && source.is_some_and(|s| same_lines(c, &k.project, s, file));
            let post = forge::CommentPost {
                file,
                line: *line,
                body,
                head: source.map(|s| s.head.as_str()),
                inline,
                check: &|text| check(c, text, &at),
            };
            let posted = forge::write(&cfg)?.comment(*pr, post)?;
            Ok(landing(&posted, file, *line))
        }
        ActionWhat::ReplyComment { pr, comment, body, .. } => {
            let k = c.card()?;
            check(c, body, "the reply on the PR")?;
            Ok(
                json!({"id": forge::write(&c.project(&k.project)?.forge)?.reply(*pr, *comment, body, &|text| check(c, text, "the reply on the PR"))?}),
            )
        }
        // Nothing of ours is sent: the comment's id alone, its thread resolved as the review bot posted it.
        ActionWhat::ResolveComment { pr, comment, .. } => {
            let k = c.card()?;
            let already = forge::write(&c.project(&k.project)?.forge)?.resolve(*pr, *comment)?;
            let mut result = json!({"resolved": comment});
            if already {
                result["already"] = json!(true);
            }
            Ok(result)
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
        | ActionWhat::Door(_) => Err(failed("not a forge action")),
    }
}

fn same_lines(c: &Call, project: &str, source: &mork_types::action::CommentSource, file: &str) -> bool {
    if source.head == source.candidate {
        return true;
    }
    let Ok(m) = c.mirror(project) else { return false };
    let contents = |at: &Sha| mirror::contents_at(&c.home, &m, at.as_str(), &[file]).ok();
    contents(&source.head).is_some_and(|head| Some(head) == contents(&source.candidate))
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
    if let Some(hit) = mirror::added_secret(&c.home, &m, &from, commit.as_str(), &secrets).map_err(failed)? {
        return Err(ActionFailure::Leak { words: format!("leak: this push would publish {hit}") });
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

// Every merge, retried too, needs the operator's approval on the head acted and `into` where its run merged
// it in (fac-98, fac-111); an older build's merge with no destination goes into the base.
fn merge(c: &Call, pr: u64, (head, base): (&Sha, &Sha), dest: Option<&str>) -> Result<Value, ActionFailure> {
    let k = c.card()?;
    let p = c.project(&k.project)?;
    if !p.forge.merges_on_approval() {
        return Err(ActionFailure::Guard { words: "This project uses manual PR merges".into(), pr: None });
    }
    let repo = c.readers.read(&p.forge)?;
    let into = dest.unwrap_or(&p.base);
    let seen = repo.observe(pr)?;
    let why = match seen.state {
        PrState::Open if seen.head != *head => {
            Some(format!("PR {pr}'s head is {}, not the approved {}", seen.head.short(), head.short()))
        }
        PrState::Open if seen.dest.as_deref() != Some(into) => {
            Some(format!("PR {pr} merges into {}, not {into}", seen.dest.clone().unwrap_or_default()))
        }
        PrState::Open if !repo.has_approver() => Some("the token file names no approver".to_owned()),
        PrState::Open if !seen.approved => {
            Some(format!("The operator's approval doesn't stand on PR {pr}'s head {}", seen.head.short()))
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
    merge_as_operator(c, &p, &k.project, pr, into, head)
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
