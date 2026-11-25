//! The Bitbucket side of the outward actions (2.5, 4.10–4.11, 4.16) and of the poller.
//! A refusal is mapped to 2.5.7's failures.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use factory_types::Sha;
use factory_types::work::{ActionFailure, OpenPr, PrSeen, PrState};
use serde_json::{Value, json};

use super::client::{Answer, BitbucketError, Repo};
use super::parse;
use crate::clock;

/// 2.5.7's failure for a refused answer: 401 is the token's, 404 a PR that's gone (F-B11).
pub fn failure(a: &Answer) -> ActionFailure {
    let words = format!("Bitbucket answered {}: {}", a.status, a.words());
    match a.status {
        401 => ActionFailure::Token { words },
        404 => ActionFailure::Gone { words },
        _ => ActionFailure::Failed { words },
    }
}

fn call(r: Result<Answer, BitbucketError>) -> Result<Answer, ActionFailure> {
    let a = r.map_err(|e| ActionFailure::Failed { words: e.to_string() })?;
    if a.ok() { Ok(a) } else { Err(failure(&a)) }
}

fn listed(e: BitbucketError) -> ActionFailure {
    match e {
        BitbucketError::Answered(a) => failure(&a),
        e @ BitbucketError::Unreachable(_) => ActionFailure::Failed { words: e.to_string() },
    }
}

/// The full hashes Bitbucket's commit call gave for its short ones, per repository: kept across
/// polls, so a head costs one call, once (fac-361).
#[derive(Debug, Clone, Default)]
pub struct Fulls(Arc<Mutex<BTreeMap<(String, String), String>>>);

impl Fulls {
    /// A commit's full hash, as kept or as Bitbucket's commit call answers; never the short one. No
    /// hash, as a PR read without a head has, stays none.
    pub fn of(&self, repo: &Repo, hash: &str) -> Result<String, ActionFailure> {
        if hash.len() == 40 || hash.is_empty() {
            return Ok(hash.to_owned());
        }
        let key = (format!("{}/{}", repo.workspace, repo.slug), hash.to_owned());
        // Held across the call, so two threads missing one head make one call: these calls are rare.
        let mut kept = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(full) = kept.get(&key) {
            return Ok(full.clone());
        }
        let a = call(repo.get(&format!("/commit/{hash}")))?;
        let full = a
            .body
            .get("hash")
            .and_then(Value::as_str)
            .filter(|h| h.len() == 40 && h.starts_with(hash))
            .map(str::to_owned)
            .ok_or_else(|| ActionFailure::Failed { words: format!("commit {hash}'s answer names no full hash") })?;
        kept.insert(key, full.clone());
        Ok(full)
    }
}

/// The open PR a branch has into `base`, if any: review-bot reads it, before Andrei's token writes
/// (fac-111).
pub fn open_of_branch(repo: &Repo, branch: &str, base: &str) -> Result<Option<Value>, ActionFailure> {
    let all = prs_of_branch(repo, branch)?;
    let into = |p: &Value| p.pointer("/destination/branch/name").and_then(Value::as_str) == Some(base);
    Ok(all.into_iter().find(|p| p.get("state").and_then(Value::as_str) == Some("OPEN") && into(p)))
}

/// A PR as Bitbucket has it: the card's own, read by number.
pub fn pr_of(repo: &Repo, pr: u64) -> Result<Value, ActionFailure> {
    Ok(call(repo.get(&format!("/pullrequests/{pr}")))?.body)
}

/// Whether a PR read is open, from `branch` into `base`: the one a card's publication means.
pub fn is_mine(pr: &Value, branch: &str, base: &str) -> bool {
    pr.get("state").and_then(Value::as_str) == Some("OPEN")
        && pr.pointer("/source/branch/name").and_then(Value::as_str) == Some(branch)
        && pr.pointer("/destination/branch/name").and_then(Value::as_str) == Some(base)
}

/// Opens a PR from a branch, titled and described (fac-111, fac-290), closing its branch on merge
/// (4.10.1); nobody is added as its reviewer.
pub fn open_pr(
    repo: &Repo,
    branch: &str,
    base: &str,
    (title, description): (&str, &str),
) -> Result<Value, ActionFailure> {
    let body = json!({
        "title": title,
        "description": description,
        "source": {"branch": {"name": branch}},
        "destination": {"branch": {"name": base}},
        "close_source_branch": true,
    });
    let a = call(repo.post("/pullrequests", &body))?;
    let pr = a
        .body
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ActionFailure::Failed { words: "the PR's answer has no id".into() })?;
    Ok(json!({"pr": pr}))
}

/// A branch's open PR published again: given the title and the description. Its reviewers are
/// sent as they stand, since the update replaces that list.
pub fn retitle(repo: &Repo, open: &Value, (title, description): (&str, &str)) -> Result<Value, ActionFailure> {
    let pr = open
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| ActionFailure::Failed { words: "the PR's listing has no id".into() })?;
    let mut body = json!({"title": title, "description": description});
    let reviewers: Vec<Value> = open
        .get("reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("uuid").map(|u| json!({"uuid": u})))
        .collect();
    if !reviewers.is_empty() {
        body["reviewers"] = Value::Array(reviewers);
    }
    call(repo.put(&format!("/pullrequests/{pr}"), &body))?;
    Ok(json!({"pr": pr}))
}

/// The PRs of a branch in every state, newest first (F-B2): what a crash's check reads.
pub fn prs_of_branch(repo: &Repo, branch: &str) -> Result<Vec<Value>, ActionFailure> {
    let q = format!(
        "/pullrequests?q=source.branch.name%3D%22{branch}%22&state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED&sort=-created_on"
    );
    repo.all(&q).map_err(listed)
}

/// Posts an inline comment, or one on the whole PR with no file (F-B9); where Bitbucket anchored it.
pub fn comment(repo: &Repo, pr: u64, file: &str, line: u64, body: &str) -> Result<Value, ActionFailure> {
    let mut req = json!({"content": {"raw": body}});
    if !file.is_empty() {
        req["inline"] = json!({"path": file, "to": line});
    }
    let a = call(repo.post(&format!("/pullrequests/{pr}/comments"), &req))?;
    Ok(landing_of(&a.body, file, line))
}

/// Posts a reply under the comment whose finding remains open.
pub fn reply(repo: &Repo, pr: u64, comment: u64, body: &str) -> Result<Value, ActionFailure> {
    let req = json!({"content": {"raw": body}, "parent": {"id": comment}});
    let a = call(repo.post(&format!("/pullrequests/{pr}/comments"), &req))?;
    Ok(json!({"id": a.body.get("id")}))
}

/// Where a comment asked at `file:line` landed, with its id: a post's answer, or a crash's check.
pub fn landing_of(c: &Value, file: &str, line: u64) -> Value {
    let path = c.pointer("/inline/path").and_then(Value::as_str).unwrap_or("");
    let to = c.pointer("/inline/to").and_then(Value::as_u64).unwrap_or(0);
    json!({"id": c.get("id"), "asked": format!("{file}:{line}"), "landed": format!("{path}:{to}")})
}

/// Resolves a comment's thread (fac-111); one resolved already, Bitbucket's 409, is resolved.
pub fn resolve(repo: &Repo, pr: u64, comment: u64) -> Result<Value, ActionFailure> {
    let a = repo.post(&format!("/pullrequests/{pr}/comments/{comment}/resolve"), &json!({}));
    let a = a.map_err(|e| ActionFailure::Failed { words: e.to_string() })?;
    match a.status {
        _ if a.ok() => Ok(json!({"resolved": comment})),
        409 => Ok(json!({"resolved": comment, "already": true})),
        _ => Err(failure(&a)),
    }
}

/// The PR's comments (F-B10).
pub fn comments(repo: &Repo, pr: u64) -> Result<Vec<Value>, ActionFailure> {
    repo.all(&format!("/pullrequests/{pr}/comments?pagelen=100")).map_err(listed)
}

/// All pages of the comments a round judges, without recovery's page limit (fac-448).
pub fn thread_comments(repo: &Repo, pr: u64) -> Result<Vec<Value>, ActionFailure> {
    let mut all = Vec::new();
    let mut next = Some(format!("/pullrequests/{pr}/comments?pagelen=100"));
    while let Some(url) = next.take() {
        let a = call(repo.get(&url))?;
        let values = a
            .body
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| ActionFailure::Failed { words: "the PR's comments answer has no values".into() })?;
        all.extend(values.iter().cloned());
        next = a.body.get("next").and_then(Value::as_str).map(str::to_owned);
    }
    Ok(all)
}

/// A finding's root comment, whose own endpoint carries its resolution (fac-448).
pub fn thread_root(repo: &Repo, pr: u64, comment: u64) -> Result<Value, ActionFailure> {
    Ok(call(repo.get(&format!("/pullrequests/{pr}/comments/{comment}")))?.body)
}

/// A comment already posted with this body at this place, as F-B10 lists them: one on the whole
/// PR has no file.
pub fn posted(all: &[Value], file: &str, line: u64, body: &str) -> Option<Value> {
    let at = |c: &Value| match c.get("inline").filter(|i| !i.is_null()) {
        None => file.is_empty(),
        Some(i) => {
            i.get("path").and_then(Value::as_str) == Some(file) && i.get("to").and_then(Value::as_u64) == Some(line)
        }
    };
    all.iter()
        .find(|c| c.pointer("/content/raw").and_then(Value::as_str) == Some(body) && at(c))
        .map(|c| landing_of(c, file, line))
}

/// Merges a PR (`merge_commit`, no message), following a 202's task to its end (F-B6); its merge commit's hash as
/// Bitbucket gives it, made full by the mirror. `close` deletes its source branch; else the PR's choice holds.
pub fn merge(repo: &Repo, pr: u64, close: bool) -> Result<String, ActionFailure> {
    let mut body = json!({"merge_strategy": "merge_commit"});
    if close {
        body["close_source_branch"] = json!(true);
    }
    let a = call(repo.post(&format!("/pullrequests/{pr}/merge"), &body))?;
    if a.status != 202 {
        return hash_of(&a.body);
    }
    let task =
        a.location.clone().ok_or_else(|| ActionFailure::Failed { words: "a 202 with no task to follow".into() })?;
    let start = clock::instant();
    while clock::elapsed(start) < Duration::from_secs(600) {
        let t = call(repo.get(&task))?;
        match t.body.get("task_status").and_then(Value::as_str) {
            Some("PENDING") => clock::sleep(Duration::from_millis(500)),
            Some("SUCCESS") => return hash_of(t.body.get("merge_result").unwrap_or(&Value::Null)),
            other => {
                return Err(ActionFailure::Failed {
                    words: format!("the merge task ended {}: {}", other.unwrap_or("?"), t.body),
                });
            }
        }
    }
    Err(ActionFailure::Failed { words: "the merge task was still pending after 10 minutes".into() })
}

fn hash_of(pr: &Value) -> Result<String, ActionFailure> {
    pr.pointer("/merge_commit/hash")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ActionFailure::Failed { words: format!("the merge's answer names no merge commit: {pr}") })
}

/// Reads a PR's state, head and merge commit, both full; approval must stand now and belong to that
/// head (fac-98). Uses paginated activity when needed for the head or approval, matching its head to the PR's.
pub fn read_pr(repo: &Repo, pr: u64, approver: &str, fulls: &Fulls) -> Result<(PrSeen, Option<String>), ActionFailure> {
    let p = call(repo.get(&format!("/pullrequests/{pr}")))?.body;
    let approves = !approver.is_empty() && parse::approves(&p, approver);
    let act = if approves || p.pointer("/source/commit/hash").is_none() {
        json!({"values": repo.all(&format!("/pullrequests/{pr}/activity?pagelen=50")).map_err(listed)?})
    } else {
        Value::Null
    };
    let state = match p.get("state").and_then(Value::as_str) {
        Some("MERGED") => PrState::Merged,
        Some("DECLINED") => PrState::Declined,
        Some("SUPERSEDED") => PrState::Superseded,
        _ => PrState::Open,
    };
    let head = p
        .pointer("/source/commit/hash")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| parse::current_head(&act))
        .unwrap_or_default();
    // The activity's head is matched in Bitbucket's own short form, before the PR's is made full.
    let approved = approves && parse::approved_on(&act, approver, &head);
    let head = fulls.of(repo, &head)?;
    let merge = p.pointer("/merge_commit/hash").and_then(Value::as_str).map(|h| fulls.of(repo, h)).transpose()?;
    let seen = PrSeen {
        pr,
        state,
        approved,
        head: Sha::new(head),
        merge_parents: Vec::new(),
        merge_commit: merge.clone().map(Sha::new),
        dest: p.pointer("/destination/branch/name").and_then(Value::as_str).map(str::to_owned),
    };
    Ok((seen, merge))
}

/// The open PRs (F-B3), their heads full; `ours` when the token's account opened them.
pub fn open_prs(repo: &Repo, me: Option<&str>, fulls: &Fulls) -> Result<Vec<OpenPr>, ActionFailure> {
    let all = repo.all("/pullrequests?state=OPEN&pagelen=50").map_err(listed)?;
    all.iter()
        .filter_map(|p| parse::open_pr(p, me))
        .map(|mut x| {
            x.head = Sha::new(fulls.of(repo, x.head.as_str())?);
            Ok(x)
        })
        .collect()
}

/// Who the token is (3.7.2): `Ok(Some(why))` when it may not act, `Ok(None)` when it may, and
/// `Err` when `GET /user` gives nothing usable (accepted: no PR opens with it, fac-111).
pub fn identity(repo: &Repo, approver: Option<&str>) -> Result<Option<String>, BitbucketError> {
    let a = repo.user()?;
    match a.status {
        401 => Ok(Some(format!("Bitbucket refuses the token (401): {}", a.words()))),
        s if (200..300).contains(&s) => Ok(approver
            .filter(|w| parse::is_account(&a.body, w))
            .map(|_| "the token is Andrei's own account".to_owned())),
        _ => Err(BitbucketError::Answered(a)),
    }
}
