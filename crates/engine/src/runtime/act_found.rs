// A `started` action as found after a crash (2.5.2's table): done, not done (so it runs), or unclear
// (so it is held).

use factory_types::Sha;
use factory_types::action::{ActionWhat, BoardEdit};
use factory_types::work::Found;
use serde_json::{Value, json};

use super::act_run::{Call, source};
use crate::bitbucket::calls;
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
        ActionWhat::CreateBoardCard { project, file, text, .. } if !file.is_empty() => {
            created_as_decided(c, project, file, text)
        }
        ActionWhat::CreateBoardCard { project, title, description, .. } => created(c, project, title, description),
        ActionWhat::ChangeBoardCard { project, file, before, after, dest, .. } if !after.is_empty() => {
            changed_as_decided(c, project, file, before, after, dest)
        }
        ActionWhat::ChangeBoardCard { project, file, edit, .. } => changed(c, project, file, edit),
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
                "{base} in Andrei's checkout already moved to {target}. Couldn't open the mirror: {e}. Retry finishes it"
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
    let Ok(repo) = super::act_bitbucket::repo(&p) else { return unclear("no token") };
    // As the call's answer would have been (M3.3), its author Andrei's by intent (fac-98); one not
    // titled and described so is run again, an update done twice alike.
    let judged = |pr: &serde_json::Value| {
        let titled = pr.get("title").and_then(|t| t.as_str()) == Some(title);
        let described = pr.get("description").and_then(|d| d.as_str()).unwrap_or_default() == description;
        if titled && described { Found::Done { result: json!({"pr": pr.get("id")}) } } else { Found::NotDone }
    };
    // The card's own PR is the one looked at: another open on its branch is nothing to it.
    if let Some(n) = expected {
        return match calls::pr_of(&repo, n) {
            Ok(pr) if calls::is_mine(&pr, branch, &p.base) => judged(&pr),
            Ok(_) => unclear(format!("PR {n} is no longer open from {branch} into {}", p.base)),
            Err(_) => unclear("Bitbucket couldn't be read"),
        };
    }
    // The PRs the branch's other openings made are theirs, not this one's (fac-382).
    let claimed: Vec<u64> = c.same_done.iter().flatten().copied().collect();
    let unclaimed = |pr: &Value| !pr.get("id").and_then(Value::as_u64).is_some_and(|id| claimed.contains(&id));
    match calls::prs_of_branch(&repo, branch) {
        Ok(prs) => match prs.iter().find(|pr| unclaimed(pr)) {
            None => Found::NotDone,
            Some(pr) if pr.get("state").and_then(|s| s.as_str()) == Some("OPEN") => judged(pr),
            Some(pr) => unclear(format!(
                "the branch's latest PR is {}: never reopened",
                pr.get("state").and_then(|s| s.as_str()).unwrap_or("?")
            )),
        },
        Err(_) => unclear("Bitbucket couldn't be read"),
    }
}

fn comment(c: &Call, pr: u64, file: &str, line: u64, body: &str) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = super::act_bitbucket::repo(&p) else { return unclear("no token") };
    let Ok(all) = calls::comments(&repo, pr) else { return unclear("Bitbucket couldn't be read") };
    // Comments come oldest first. Those other done actions posted, another finding's or an earlier
    // raising of this one's (fac-111), aren't this one's: by the ids kept, else the oldest left (R18).
    let ids: Vec<u64> = c.same_done.iter().flatten().copied().collect();
    let unknown = c.same_done.iter().filter(|x| x.is_none()).count();
    let theirs = |x: &Value| x.get("id").and_then(Value::as_u64).is_some_and(|id| ids.contains(&id));
    let free: Vec<Value> = all
        .into_iter()
        .filter(|x| x.pointer("/content/raw").and_then(Value::as_str) == Some(body) && !theirs(x))
        .skip(unknown)
        .collect();
    if let Some(result) = calls::posted(&free, file, line, body) {
        return Found::Done { result };
    }
    // The same body somewhere else: Bitbucket anchored it elsewhere, its id kept as a post keeps it.
    match free.as_slice() {
        [] => Found::NotDone,
        [one] => Found::Done { result: calls::landing_of(one, file, line) },
        _ => unclear("the same comment is on the PR more than once"),
    }
}

fn reply(c: &Call, pr: u64, comment: u64, body: &str) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = super::act_bitbucket::repo(&p) else { return unclear("no token") };
    let tracked = k.review.as_ref().is_some_and(|r| {
        r.raised.iter().any(|f| f.answer.as_ref() == Some(&c.action.id) || f.dropped.as_ref() == Some(&c.action.id))
    });
    let read = if tracked { calls::thread_comments(&repo, pr) } else { calls::comments(&repo, pr) };
    let Ok(all) = read else { return unclear("Bitbucket couldn't be read") };
    let posted = all.iter().find(|x| {
        x.pointer("/parent/id").and_then(Value::as_u64) == Some(comment)
            && x.pointer("/content/raw").and_then(Value::as_str) == Some(body)
    });
    match posted {
        Some(x) => Found::Done { result: json!({"id": x.get("id")}) },
        None => Found::NotDone,
    }
}

fn merge(c: &Call, pr: u64, head: &factory_types::Sha) -> Found {
    let Some(k) = c.card.as_ref() else { return unclear("the card is gone") };
    let Ok(p) = c.project(&k.project) else { return unclear("no project") };
    let Ok(repo) = super::act_bitbucket::repo(&p) else { return unclear("no token") };
    let approver = super::act_bitbucket::approver().unwrap_or_default();
    match calls::read_pr(&repo, pr, &approver, &c.fulls) {
        Ok((seen, Some(hash))) if seen.state == factory_types::work::PrState::Merged => {
            Found::Done { result: super::act_bitbucket::verify(c, &p, &k.project, &hash, head) }
        }
        Ok((seen, _)) if seen.state == factory_types::work::PrState::Open => Found::NotDone,
        Ok((seen, _)) => unclear(format!("PR {pr} is {:?}", seen.state)),
        Err(_) => unclear("Bitbucket couldn't be read"),
    }
}

// Other content is held, nothing written (2.5).
fn created_as_decided(c: &Call, project: &str, file: &str, text: &str) -> Found {
    let Ok(b) = super::act_board::backlog(c, project) else { return unclear("no board") };
    match std::fs::read_to_string(b.join(file)) {
        Ok(now) if now == text => Found::Done { result: json!({"id": super::act_board::id_of(file), "file": file}) },
        Ok(_) => unclear(format!("{file} holds another card's text")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Found::NotDone,
        Err(e) => unclear(format!("{file}: {e}")),
    }
}

// Content neither before nor after the change is held (2.5).
fn changed_as_decided(c: &Call, project: &str, file: &str, before: &str, after: &str, dest: &str) -> Found {
    let Ok(b) = super::act_board::backlog(c, project) else { return unclear("no board") };
    let hash = |f: &str| std::fs::read_to_string(b.join(f)).ok().map(|t| factory_types::hash::short(&t));
    if hash(dest).as_deref() == Some(after) {
        return Found::Done { result: json!({"file": dest, "before": before, "after": after}) };
    }
    if hash(file).as_deref() == Some(before) {
        return Found::NotDone;
    }
    unclear(format!("{file} holds neither what it held before the change nor the change"))
}

fn created(c: &Call, project: &str, title: &str, description: &str) -> Found {
    let Ok(b) = super::act_board::backlog(c, project) else { return unclear("no board") };
    let found = std::fs::read_dir(b.join("tasks")).into_iter().flatten().flatten().find_map(|e| {
        let text = std::fs::read_to_string(e.path()).ok()?;
        let f = crate::board::parse(&text).ok()?;
        (f.title == title && f.description == description.trim())
            .then(|| (f.id, e.file_name().to_string_lossy().into_owned()))
    });
    match found {
        Some((id, name)) => Found::Done { result: json!({"id": id, "file": format!("tasks/{name}")}) },
        None => Found::NotDone,
    }
}

// A change already made shows its words in the file (its note, its fields); a closed card is
// where it was moved.
fn changed(c: &Call, project: &str, file: &str, edit: &BoardEdit) -> Found {
    let Ok(b) = super::act_board::backlog(c, project) else { return unclear("no board") };
    let name = std::path::Path::new(file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read_to_string(b.join(file)).unwrap_or_default();
    // What the file held when the change started, and what it writes (2.5.2's table).
    if let Some(p) = c.action.result.as_ref().filter(|_| !matches!(edit, BoardEdit::Close { .. })) {
        let now = crate::files::sha256_hex(text.as_bytes());
        return if p.get("after").and_then(|v| v.as_str()) == Some(now.as_str()) {
            Found::Done { result: json!({"file": file}) }
        } else if p.get("before").and_then(|v| v.as_str()) == Some(now.as_str()) {
            Found::NotDone
        } else {
            unclear(format!("{file} holds neither what it held before the change nor the change"))
        };
    }
    let has = |s: &str| text.contains(s);
    let done = match edit {
        BoardEdit::Note { text: t, .. } => has(t),
        BoardEdit::Fields { title, status, .. } => {
            title.as_deref().is_none_or(has) && status.as_deref().is_none_or(|s| has(&format!("status: {s}")))
        }
        BoardEdit::Close { done, .. } => {
            let moved = if *done { b.join("completed").join(&name) } else { b.join("archive/tasks").join(&name) };
            moved.exists() && !b.join(file).exists()
        }
    };
    if done {
        return Found::Done { result: json!({"file": file}) };
    }
    if b.join(file).exists() { Found::NotDone } else { unclear(format!("{file} is gone")) }
}
