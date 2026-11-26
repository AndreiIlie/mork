// A `started` action as found after a crash (2.5.2's table): done, not done (so it runs), or unclear
// (so it is held).

use mork_types::Sha;
use mork_types::action::ActionWhat;
use mork_types::work::Found;
use serde_json::json;

use super::act_run::{Call, source};
use crate::forge;
use crate::git::{checkout, mirror};

fn unclear(words: impl Into<String>) -> Found {
    Found::Unclear { words: words.into() }
}

/// Checks the world for a `started` action.
pub fn found(c: &Call) -> Found {
    match &c.action.what {
        // Repeating a read, a removal or a non-forced backup of the current base is harmless.
        ActionWhat::Fetch { .. }
        | ActionWhat::BackupPush { .. }
        | ActionWhat::ResolveComment { .. }
        | ActionWhat::Take { .. }
        | ActionWhat::TakeJoin { .. }
        | ActionWhat::RemoveClone { .. }
        | ActionWhat::RemoveTrain { .. } => Found::NotDone,
        ActionWhat::Push { branch, commit, .. } => push(c, branch, commit.as_str()),
        ActionWhat::OpenPr { project, branch, title, description, pr, .. } => {
            open_pr(c, project, (branch, title, description), *pr)
        }
        ActionWhat::Comment { pr, file, line, body, .. } => comment(c, *pr, file, *line, body),
        ActionWhat::ReplyComment { pr, comment, body, .. } => reply(c, *pr, *comment, body),
        ActionWhat::Merge { pr, head, .. } => merge(c, *pr, head),
        ActionWhat::MoveBase { project, base, from, target, .. } => move_base(c, project, base, (from, target)),
        ActionWhat::CreateBoardCard { project, .. } | ActionWhat::ChangeBoardCard { project, .. } => {
            use crate::board::BoardRead;
            let Ok(p) = c.project(project) else { return unclear("no board") };
            crate::board::read(&p.board).found(&c.action)
        }
        ActionWhat::Door(_) => crate::door::call::found(&c.home, &c.bin, &c.action, &c.door_redactor()),
    }
}

// Recovery cannot publish the new tip to live clones until the mirror has caught up (fac-613).
fn move_base(c: &Call, project: &str, base: &str, (from, target): (&Sha, &Sha)) -> Found {
    let Ok(p) = c.project(project) else { return unclear("the project is gone") };
    let result = match checkout::found(&c.home, std::path::Path::new(&p.repo), base, from.as_str(), target.as_str()) {
        Ok(Some(result)) => result,
        Ok(None) => return Found::NotDone,
        Err(words) => return unclear(words),
    };
    let m = match mirror::ensure(&c.home, project) {
        Ok(m) => m,
        Err(e) => {
            return unclear(format!(
                "{base} in the operator's checkout already moved to {target}. Couldn't open the mirror: {e}. Retry finishes it"
            ));
        }
    };
    match checkout::follow(&c.home, &m, base, target.as_str()) {
        Ok(()) => Found::Done { result },
        Err(words) => unclear(words),
    }
}

fn push(c: &Call, branch: &str, commit: &str) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let (Ok(p), Ok(m)) = (c.project(&k.project), c.mirror(&k.project)) else { return unclear("no project") };
    let Ok((to, env)) = source(&p) else { return unclear("no origin") };
    match mirror::ls_remote(&c.home, &m, &to, branch, &env) {
        Ok(Some(at)) if at == commit => Found::Done { result: json!({"pushed": commit}) },
        Ok(_) => Found::NotDone,
        Err(words) => unclear(format!("origin couldn't be read: {words}")),
    }
}

fn open_pr(c: &Call, project: &str, (branch, title, description): (&str, &str, &str), expected: Option<u64>) -> Found {
    let Ok(p) = c.project(project) else { return unclear("no project") };
    let Ok(repo) = c.readers.read(&p.forge) else { return unclear("no token") };
    // As the call's answer would have been (M3.3), its author the operator's by intent (fac-98); one not
    // titled and described so is run again, an update done twice alike.
    let judged = |pr: &forge::Pr| {
        let titled = pr.title.as_deref() == Some(title);
        let described = pr.description == description;
        if titled && described { Found::Done { result: json!({"pr": pr.id}) } } else { Found::NotDone }
    };
    // The card's own PR is the one looked at: another open on its branch is nothing to it.
    if let Some(n) = expected {
        return match repo.pr(n) {
            Ok(pr) if pr.open && pr.branch.as_deref() == Some(branch) && pr.dest.as_deref() == Some(&p.base) => {
                judged(&pr)
            }
            Ok(_) => unclear(format!("PR {n} is no longer open from {branch} into {}", p.base)),
            Err(_) => unclear(format!("{} couldn't be read", p.forge.label())),
        };
    }
    // The PRs the branch's other openings made are theirs, not this one's (fac-382).
    let claimed: Vec<u64> = c.same_done.iter().flatten().copied().collect();
    let unclaimed = |pr: &forge::Pr| !pr.id.is_some_and(|id| claimed.contains(&id));
    match repo.branch_prs(branch) {
        Ok(prs) => match prs.iter().find(|pr| unclaimed(pr)) {
            None => Found::NotDone,
            Some(pr) if pr.open => judged(pr),
            Some(pr) => unclear(format!("the branch's latest PR is {}: never reopened", pr.state)),
        },
        Err(_) => unclear(format!("{} couldn't be read", p.forge.label())),
    }
}

fn comment(c: &Call, pr: u64, file: &str, line: u64, body: &str) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = c.readers.read(&p.forge) else { return unclear("no token") };
    let Ok(all) = repo.comments(pr, false) else { return unclear(format!("{} couldn't be read", p.forge.label())) };
    // Comments come oldest first. Those other done actions posted, another finding's or an earlier
    // raising of this one's (fac-111), aren't this one's: by the ids kept, else the oldest left (R18).
    let ids: Vec<u64> = c.same_done.iter().flatten().copied().collect();
    let unknown = c.same_done.iter().filter(|x| x.is_none()).count();
    let theirs = |x: &forge::Comment| x.id.is_some_and(|id| ids.contains(&id));
    let free: Vec<_> =
        all.into_iter().filter(|x| x.body.as_deref() == Some(body) && !theirs(x)).skip(unknown).collect();
    let at = |x: &&forge::Comment| match x.requested.as_ref().or(x.inline.as_ref()) {
        None => file.is_empty(),
        Some(a) => a.path.as_deref() == Some(file) && a.line == Some(line),
    };
    if let Some(posted) = free.iter().find(at) {
        return Found::Done { result: super::act_forge::landing(posted, file, line) };
    }
    // The same body somewhere else: Bitbucket anchored it elsewhere, its id kept as a post keeps it.
    match free.as_slice() {
        [] => Found::NotDone,
        [one] => Found::Done { result: super::act_forge::landing(one, file, line) },
        _ => unclear("the same comment is on the PR more than once"),
    }
}

fn reply(c: &Call, pr: u64, comment: u64, body: &str) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = c.readers.read(&p.forge) else { return unclear("no token") };
    let tracked = k.review.as_ref().is_some_and(|r| {
        r.raised.iter().any(|f| f.answer.as_ref() == Some(&c.action.id) || f.dropped.as_ref() == Some(&c.action.id))
    });
    let read = repo.comments(pr, tracked);
    let Ok(all) = read else { return unclear(format!("{} couldn't be read", p.forge.label())) };
    let posted = all.iter().find(|x| x.parent == Some(comment) && x.body.as_deref() == Some(body));
    match posted {
        Some(x) => Found::Done { result: json!({"id": x.id}) },
        None => Found::NotDone,
    }
}

fn merge(c: &Call, pr: u64, head: &mork_types::Sha) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = c.readers.read(&p.forge) else { return unclear("no token") };
    match repo.observe(pr) {
        Ok(seen) if seen.state == mork_types::work::PrState::Merged => match seen.merge_commit {
            Some(hash) => Found::Done { result: super::act_forge::verify(c, &p, &k.project, hash.as_str(), head) },
            None => unclear(format!("PR {pr} is Merged")),
        },
        Ok(seen) if seen.state == mork_types::work::PrState::Open => Found::NotDone,
        Ok(seen) => unclear(format!("PR {pr} is {:?}", seen.state)),
        Err(_) => unclear(format!("{} couldn't be read", p.forge.label())),
    }
}
