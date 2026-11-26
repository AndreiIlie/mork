// why: malformed parser fixtures should fail the test at the missing field.
#![allow(clippy::unwrap_used)]
use super::{comments, parse, threads};
use serde_json::json;

#[test]
fn merged_commit_and_source_repository() {
    let mut p = json!({"number":4,"state":"open","head":{"sha":"abc","repo":{"full_name":"Owner/RePo"}},"base":{"ref":"main"},"merge_commit_sha":"tested"});
    assert!(parse::same_repo(&p, "owner", "repo"));
    assert!(parse::seen(&p).unwrap().merge_commit.is_none());
    p["head"]["repo"] = json!(null);
    assert!(!parse::same_repo(&p, "owner", "repo"));
    p["state"] = json!("closed");
    assert_eq!(parse::seen(&p).unwrap().state, mork_types::work::PrState::Declined);
    p["merged"] = json!(true);
    assert_eq!(parse::seen(&p).unwrap().merge_commit.unwrap().as_str(), "tested");
}

#[test]
fn approval_stands_on_its_commit_until_withdrawn() {
    let review = |state: &str| json!({"user":{"login":"Owner"},"commit_id":"abc","state":state});
    let mut reviews = vec![review("APPROVED"), review("COMMENTED")];
    assert!(parse::approved(&reviews, "owner", "abc"));
    assert!(!parse::approved(&reviews, "owner", "other"));
    reviews.push(review("CHANGES_REQUESTED"));
    assert!(!parse::approved(&reviews, "owner", "abc"));
    reviews.push(review("DISMISSED"));
    assert!(!parse::approved(&reviews, "owner", "abc"));
}

#[test]
fn recovers_comment_kinds_locations_and_replies() {
    let review =
        comments::parse(&json!({"id":10,"in_reply_to_id":8,"body":"found","path":"file","line":9}), false).unwrap();
    assert_eq!(review.parent, Some(16));
    assert_eq!(review.inline.unwrap().line, Some(9));
    let issue = comments::parse(&json!({"id":10,"body":"file:9\n\nfound"}), true).unwrap();
    assert_ne!(issue.id, review.id);
    assert_eq!(issue.body.as_deref(), Some("found"));
    assert_eq!(issue.inline.unwrap().path.as_deref(), Some("file"));
    let reply = comments::parse(&json!({"id":11,"body":"In reply to [this comment](#issuecomment-10)\n\nfixed"}), true)
        .unwrap();
    assert_eq!(reply.parent, issue.id);
    assert_eq!(reply.body.as_deref(), Some("fixed"));
}

#[test]
fn matches_resolved_threads_using_full_comment_ids() {
    let t = json!({"isResolved":true,"resolvedBy":{"login":"owner"},"comments":{"nodes":[{"fullDatabaseId":"4000000000","author":{"login":"bot"}}]}});
    let id = threads::root_id(&t).unwrap();
    assert_eq!(id, 8_000_000_000);
    assert_eq!(threads::thread(id, &t, &[]).unwrap().resolved.unwrap().by, "owner");
}
