use super::Client;
use super::client::failed;
use crate::forge::{Anchor, Comment, CommentPost, Result};
use serde_json::{Value, json};

// GitHub allocates review and conversation comment IDs in separate namespaces.
pub(super) fn key(raw: u64, issue: bool) -> Result<u64> {
    raw.checked_mul(2).map(|n| n + u64::from(issue)).ok_or_else(|| failed("GitHub comment id too large"))
}

pub(super) fn raw_id(row: &Value) -> Result<u64> {
    row["id"].as_u64().ok_or_else(|| failed("GitHub comment lacks its id"))
}

fn fallback_text(file: &str, line: u64, body: &str) -> String {
    format!("{file}:{line}\n\n{body}")
}

fn reply_text(parent: u64, body: &str) -> String {
    format!("In reply to [this comment](#issuecomment-{parent})\n\n{body}")
}

pub(super) fn parse(row: &Value, issue: bool) -> Result<Comment> {
    let id = key(raw_id(row)?, issue)?;
    let mut body = row["body"].as_str().ok_or_else(|| failed("GitHub comment lacks its body"))?;
    let mut parent = row["in_reply_to_id"].as_u64().map(|id| key(id, false)).transpose()?;
    let mut requested = None;
    let inline = row["path"]
        .as_str()
        .map(|p| Anchor { path: Some(p.into()), line: row["line"].as_u64().or_else(|| row["original_line"].as_u64()) });
    if parent.is_none()
        && (issue || row["subject_type"] == "file")
        && let Some((prefix, rest)) = body.split_once("\n\n")
    {
        if let Some(raw) = prefix
            .strip_prefix("In reply to [this comment](#issuecomment-")
            .and_then(|s| s.strip_suffix(')'))
            .and_then(|s| s.parse().ok())
        {
            parent = Some(key(raw, true)?);
            body = rest;
        } else if let Some((path, line)) = prefix.rsplit_once(':')
            && let Ok(line) = line.parse()
        {
            requested = Some(Anchor { path: Some(path.into()), line: Some(line) });
            body = rest;
        }
    }
    Ok(Comment { id: Some(id), parent, body: Some(body.into()), inline, requested })
}

pub(super) fn list(client: &Client, pr: u64) -> Result<Vec<Comment>> {
    let me = client.login()?;
    let mut rows = Vec::new();
    for (kind, issue) in [("pulls", false), ("issues", true)] {
        for row in client.all(&format!("/{kind}/{pr}/comments?per_page=100"))? {
            if row["user"]["login"].as_str().is_some_and(|login| login.eq_ignore_ascii_case(&me)) {
                rows.push((row["created_at"].as_str().unwrap_or_default().to_owned(), parse(&row, issue)?));
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(rows.into_iter().map(|(_, c)| c).collect())
}

fn issue_comment(client: &Client, pr: u64, body: &str) -> Result<Comment> {
    let url = client.repo_url(&format!("/issues/{pr}/comments"));
    let row = client.send("POST", &url, Some(&json!({"body":body})))?.value()?;
    Ok(Comment {
        id: Some(key(raw_id(&row)?, true)?),
        body: Some(body.into()),
        parent: None,
        inline: None,
        requested: None,
    })
}

pub(super) fn post(client: &Client, pr: u64, post: CommentPost<'_>) -> Result<Comment> {
    if post.file.is_empty() {
        return issue_comment(client, pr, post.body);
    }
    let url = client.repo_url(&format!("/pulls/{pr}/comments"));
    if let Some(head) = post.head
        && post.inline
        && post.line > 0
    {
        let body = json!({"body":post.body,"path":post.file,"commit_id":head,"side":"RIGHT","line":post.line});
        let answer = client.send("POST", &url, Some(&body))?;
        if answer.status != 422 {
            return parse(&answer.value()?, false);
        }
    }
    let fallback = fallback_text(post.file, post.line, post.body);
    (post.check)(&fallback)?;
    if let Some(head) = post.head {
        let file = json!({"body":fallback,"path":post.file,"commit_id":head,"subject_type":"file"});
        let answer = client.send("POST", &url, Some(&file))?;
        if answer.status != 422 {
            return parse(&answer.value()?, false);
        }
    }
    issue_comment(client, pr, &fallback)
}

pub(super) fn reply(
    client: &Client,
    pr: u64,
    parent: u64,
    body: &str,
    check: &dyn Fn(&str) -> Result<()>,
) -> Result<u64> {
    let raw = parent / 2;
    if parent % 2 == 1 {
        let text = reply_text(raw, body);
        check(&text)?;
        return issue_comment(client, pr, &text)?.id.ok_or_else(|| failed("Missing GitHub reply id"));
    }
    let url = client.repo_url(&format!("/pulls/{pr}/comments/{raw}/replies"));
    let row = client.send("POST", &url, Some(&json!({"body":body})))?.value()?;
    key(raw_id(&row)?, false)
}
