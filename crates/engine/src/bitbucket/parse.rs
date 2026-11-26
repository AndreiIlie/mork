use mork_types::Sha;
use mork_types::review::{Thread, ThreadReply, ThreadResolution};
use mork_types::work::OpenPr;
use serde_json::Value;

fn updates(activity: &Value) -> Vec<(&str, &str)> {
    activity
        .get("values")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|v| {
                    let u = v.get("update")?;
                    Some((u.get("date")?.as_str()?, u.pointer("/source/commit/hash")?.as_str()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The PR's current head, as its activity shows it: the source commit of its latest update.
pub fn current_head(activity: &Value) -> Option<String> {
    updates(activity).first().map(|(_, h)| (*h).to_owned())
}

/// Whether an account is the approver: by `{uuid}`, account id or nickname.
pub fn is_account(user: &Value, who: &str) -> bool {
    ["uuid", "account_id", "nickname"].iter().any(|k| user.get(*k).and_then(Value::as_str) == Some(who))
}

/// Whether the approver's approval stands on the current head (4.11.1): dated after the update
/// that made the current head the PR's source.
pub fn approved(activity: &Value, approver: &str) -> bool {
    let all = updates(activity);
    let Some((_, head)) = all.first() else { return false };
    let since = all.iter().take_while(|(_, h)| h == head).last().map_or("", |(d, _)| *d);
    activity.get("values").and_then(Value::as_array).is_some_and(|vals| {
        vals.iter().filter_map(|v| v.get("approval")).any(|a| {
            a.get("user").is_some_and(|u| is_account(u, approver))
                && a.get("date").and_then(Value::as_str).is_some_and(|d| d > since)
        })
    })
}

/// Whether the approver's approval stands on `head`, the PR's own as Bitbucket gives it: its
/// activity's current head is that one, so an activity lagging behind the PR approves nothing (fac-98).
pub fn approved_on(activity: &Value, approver: &str, head: &str) -> bool {
    current_head(activity).is_some_and(|h| h == head) && approved(activity, approver)
}

/// Whether the approver approves the PR now, as its participants say (F-B4): a withdrawn
/// approval is no longer theirs (fac-98). On which head it was given is the activity's to say.
pub fn approves(pr: &Value, approver: &str) -> bool {
    pr.get("participants").and_then(Value::as_array).is_some_and(|all| {
        all.iter().any(|x| {
            x.get("approved").and_then(Value::as_bool) == Some(true)
                && x.get("user").is_some_and(|u| is_account(u, approver))
        })
    })
}

/// One PR of the open PRs' list (F-B3); `ours` when `me`, the token's account, opened it.
pub fn open_pr(p: &Value, me: Option<&str>) -> Option<OpenPr> {
    let text = |at: &str| p.pointer(at).and_then(Value::as_str).unwrap_or("").to_owned();
    let author = p.get("author")?;
    Some(OpenPr {
        review_requested: false,
        pr: p.get("id")?.as_u64()?,
        author: author
            .get("nickname")
            .or_else(|| author.get("display_name"))
            .and_then(Value::as_str)
            .unwrap_or("someone")
            .to_owned(),
        title: text("/title"),
        branch: text("/source/branch/name"),
        head: Sha::new(text("/source/commit/hash")),
        fork: false,
        ours: me.is_some_and(|m| is_account(author, m)),
        dest: text("/destination/branch/name"),
        updated: text("/updated_on"),
        link: text("/links/html/href"),
        description: text("/description"),
        tickets: Vec::new(),
    })
}

fn same_account(owner: &Value, user: &Value) -> Result<bool, String> {
    ["uuid", "account_id"]
        .iter()
        .find_map(|key| {
            let a = owner.get(*key)?.as_str().filter(|s| !s.is_empty())?;
            let b = user.get(*key)?.as_str().filter(|s| !s.is_empty())?;
            Some(a == b)
        })
        .ok_or_else(|| "the comment authors have no comparable account id".into())
}

fn author(user: &Value) -> String {
    user.get("display_name").or_else(|| user.get("nickname")).and_then(Value::as_str).unwrap_or("unknown").to_owned()
}

/// A root's descendant replies at any depth and its resolution, excluding the root author's words (fac-450).
pub fn thread(root: &Value, all: &[Value]) -> Result<Thread, String> {
    use std::collections::{BTreeMap, BTreeSet};

    let owner = &root["user"];
    let id = root["id"].as_u64().ok_or("the root comment has no id")?;
    let parents: BTreeMap<_, _> =
        all.iter().filter_map(|c| Some((c["id"].as_u64()?, c.pointer("/parent/id")?.as_u64()?))).collect();
    let mut descendants = BTreeSet::from([id]);
    for child in parents.keys().copied() {
        let mut next = Some(child);
        let mut seen = BTreeSet::new();
        while let Some(at) = next {
            if descendants.contains(&at) {
                descendants.extend(seen);
                break;
            }
            if !seen.insert(at) {
                break;
            }
            next = parents.get(&at).copied();
        }
    }
    let mut replies = Vec::new();
    for reply in
        all.iter().filter(|c| c.pointer("/parent/id").and_then(Value::as_u64).is_some_and(|p| descendants.contains(&p)))
    {
        if reply["deleted"].as_bool() == Some(true) || same_account(owner, &reply["user"])? {
            continue;
        }
        replies.push(ThreadReply {
            id: reply["id"].as_u64().ok_or("a reply has no id")?,
            by: author(&reply["user"]),
            at: reply["created_on"].as_str().unwrap_or_default().to_owned(),
            text: reply.pointer("/content/raw").and_then(Value::as_str).ok_or("a reply has no text")?.to_owned(),
        });
    }
    let resolved = match root.get("resolution").filter(|r| !r.is_null()) {
        Some(r) if !same_account(owner, &r["user"])? => Some(ThreadResolution {
            by: author(&r["user"]),
            at: r["created_on"].as_str().unwrap_or_default().to_owned(),
        }),
        _ => None,
    };
    Ok(Thread { replies, resolved })
}
