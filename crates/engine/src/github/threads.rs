use super::{
    Client,
    client::{API, failed},
    comments,
    parse::string,
};
use crate::forge::Result;
use mork_types::review::{Thread, ThreadReply, ThreadResolution, Threads};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const QUERY: &str = "query($owner:String!,$repo:String!,$pr:Int!,$after:String){repository(owner:$owner,name:$repo){pullRequest(number:$pr){reviewThreads(first:100,after:$after){pageInfo{hasNextPage endCursor} nodes{id isResolved resolvedBy{login} comments(first:1){nodes{fullDatabaseId author{login}}}}}}}}";
const RESOLVE: &str = "mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{isResolved}}}";

fn graphql(client: &Client, query: &str, variables: Value) -> Result<Value> {
    let url = format!("{API}graphql");
    let body = client.send("POST", &url, Some(&json!({"query":query,"variables":variables})))?.value()?;
    if let Some(error) = body["errors"].as_array().and_then(|a| a.first()) {
        let words = client.mask(error["message"].as_str().unwrap_or("GraphQL request failed"));
        return Err(failed(format!("GitHub: {words}")));
    }
    Ok(body)
}

pub(super) fn root_id(thread: &Value) -> Result<u64> {
    let id = &thread["comments"]["nodes"][0]["fullDatabaseId"];
    let raw = id
        .as_u64()
        .or_else(|| id.as_str()?.parse().ok())
        .ok_or_else(|| failed("GitHub thread lacks its root comment"))?;
    comments::key(raw, false)
}

fn roots(client: &Client, pr: u64) -> Result<BTreeMap<u64, Value>> {
    let mut out = BTreeMap::new();
    let mut after = Value::Null;
    loop {
        let response = graphql(client, QUERY, json!({"owner":client.owner,"repo":client.repo,"pr":pr,"after":after}))?;
        let threads = &response["data"]["repository"]["pullRequest"]["reviewThreads"];
        for t in threads["nodes"].as_array().ok_or_else(|| failed("Missing GitHub review threads"))? {
            out.insert(root_id(t)?, t.clone());
        }
        if threads["pageInfo"]["hasNextPage"] == false {
            return Ok(out);
        }
        after = Value::String(string(threads, "/pageInfo/endCursor")?.into());
    }
}

pub(super) fn resolve(client: &Client, pr: u64, id: u64) -> Result<bool> {
    if id % 2 == 1 {
        return Ok(true);
    }
    let all = roots(client, pr)?;
    let root = all.get(&id).ok_or_else(|| failed("GitHub review thread was not found"))?;
    if root["isResolved"] == true {
        return Ok(true);
    }
    let response = graphql(client, RESOLVE, json!({"id":string(root,"/id")?}))?;
    if response["data"]["resolveReviewThread"]["thread"]["isResolved"] != true {
        return Err(failed("GitHub did not resolve the thread"));
    }
    Ok(false)
}

pub(super) fn thread(id: u64, root: &Value, all: &[Value]) -> Result<Thread> {
    let author = string(root, "/comments/nodes/0/author/login")?;
    let mut replies = Vec::new();
    for row in all.iter().filter(|r| r["in_reply_to_id"].as_u64() == Some(id / 2)) {
        let by = string(row, "/user/login")?;
        if !by.eq_ignore_ascii_case(author) {
            replies.push(ThreadReply {
                id: comments::key(comments::raw_id(row)?, false)?,
                by: by.into(),
                at: string(row, "/created_at")?.into(),
                text: string(row, "/body")?.into(),
            });
        }
    }
    let resolved = root["resolvedBy"]["login"]
        .as_str()
        .filter(|by| root["isResolved"] == true && !by.eq_ignore_ascii_case(author))
        .map(|by| ThreadResolution { by: by.into(), at: String::new() });
    Ok(Thread { replies, resolved })
}

pub(super) fn read(client: &Client, pr: u64, ids: &[(u32, u64)]) -> Threads {
    let data = ids
        .iter()
        .any(|(_, id)| id % 2 == 0)
        .then(|| {
            Ok::<_, mork_types::work::ActionFailure>((
                roots(client, pr)?,
                client.all(&format!("/pulls/{pr}/comments?per_page=100"))?,
            ))
        })
        .transpose();
    ids.iter()
        .map(|(n, id)| {
            let read = if id % 2 == 1 {
                Ok(Thread::default())
            } else {
                match &data {
                    Ok(Some((roots, rows))) => roots
                        .get(id)
                        .ok_or_else(|| "GitHub thread was not found".to_owned())
                        .and_then(|root| thread(*id, root, rows).map_err(|e| format!("{e:?}"))),
                    Err(e) => Err(format!("{e:?}")),
                    Ok(None) => Err("Missing GitHub review threads".into()),
                }
            };
            (*n, read)
        })
        .collect()
}
