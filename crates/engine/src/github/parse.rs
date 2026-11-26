use super::client::failed;
use crate::forge::{Pr, Result};
use mork_types::Sha;
use mork_types::work::{OpenPr, PrSeen, PrState};
use serde_json::Value;

pub(super) fn string<'a>(value: &'a Value, path: &str) -> Result<&'a str> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| failed(format!("GitHub response lacks {path}")))
}

pub(super) fn number(p: &Value) -> Result<u64> {
    p["number"].as_u64().ok_or_else(|| failed("GitHub response lacks a PR number"))
}

pub(super) fn same_repo(p: &Value, owner: &str, repo: &str) -> bool {
    p.pointer("/head/repo/full_name")
        .and_then(Value::as_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(&format!("{owner}/{repo}")))
}

pub(super) fn open(p: &Value, login: &str, tag: Option<&str>, fork: bool) -> Result<OpenPr> {
    let title = string(p, "/title")?;
    let author = string(p, "/user/login")?;
    Ok(OpenPr {
        fork,
        review_requested: tag.is_some_and(|tag| title.contains(tag)),
        pr: number(p)?,
        author: author.into(),
        title: title.into(),
        branch: string(p, "/head/ref")?.into(),
        head: Sha::new(string(p, "/head/sha")?),
        ours: author.eq_ignore_ascii_case(login),
        dest: string(p, "/base/ref")?.into(),
        updated: string(p, "/updated_at")?.into(),
        link: string(p, "/html_url")?.into(),
        description: p["body"].as_str().unwrap_or_default().into(),
        tickets: vec![],
    })
}

fn state(p: &Value) -> Result<PrState> {
    match string(p, "/state")? {
        "open" => Ok(PrState::Open),
        "closed" if p["merged"].as_bool() == Some(true) || p["merged_at"].is_string() => Ok(PrState::Merged),
        "closed" => Ok(PrState::Declined),
        _ => Err(failed("Unknown GitHub PR state")),
    }
}

pub(super) fn metadata(p: &Value) -> Result<Pr> {
    let state = state(p)?;
    Ok(Pr {
        id: Some(number(p)?),
        title: Some(string(p, "/title")?.into()),
        description: p["body"].as_str().unwrap_or_default().into(),
        open: state == PrState::Open,
        state: format!("{state:?}"),
        branch: Some(string(p, "/head/ref")?.into()),
        dest: Some(string(p, "/base/ref")?.into()),
    })
}

pub(super) fn seen(p: &Value) -> Result<PrSeen> {
    let state = state(p)?;
    let merge_commit = if state == PrState::Merged { Some(Sha::new(string(p, "/merge_commit_sha")?)) } else { None };
    Ok(PrSeen {
        pr: number(p)?,
        state,
        head: Sha::new(string(p, "/head/sha")?),
        approved: false,
        merge_parents: vec![],
        merge_commit,
        dest: Some(string(p, "/base/ref")?.into()),
    })
}

pub(super) fn approved(reviews: &[Value], account: &str, head: &str) -> bool {
    reviews
        .iter()
        .rev()
        .filter(|r| r.pointer("/user/login").and_then(Value::as_str).is_some_and(|s| s.eq_ignore_ascii_case(account)))
        .find(|r| !matches!(r["state"].as_str(), Some("COMMENTED" | "PENDING")))
        .is_some_and(|r| r["state"] == "APPROVED" && r["commit_id"] == head)
}
