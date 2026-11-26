mod calls;
mod client;
mod parse;
mod token;

use crate::forge::{Anchor, Comment, ForgeRead, ForgeWrite, Identity, Pr, Result};
pub(crate) use calls::Fulls;
use mork_types::review::Threads;
use mork_types::work::{ActionFailure, OpenPr, PrSeen};
use serde_json::Value;
pub(crate) use token::pinned;

pub(crate) fn source(workspace: &str, slug: &str) -> std::result::Result<(String, Vec<(String, String)>), String> {
    Ok((format!("https://bitbucket.org/{workspace}/{slug}.git"), crate::git::token_env(&token::read()?.git_header())))
}

pub(crate) fn token_text() -> Result<String> {
    token::read_text().map_err(|e| ActionFailure::Token { words: e.to_string() })
}

pub(crate) fn add_secrets(s: &mut crate::secrets::Secrets) {
    if let Ok(t) = token::read() {
        s.add("the Bitbucket token", &t.token);
    }
    if let Ok(t) = token::read_merge() {
        s.add("The operator's Bitbucket token", &t.token);
    }
}

pub(crate) struct Client {
    repo: client::Repo,
    token: token::Token,
}

pub(crate) struct Reader {
    client: Client,
    review_tag: Option<String>,
    fulls: Fulls,
}

impl Reader {
    pub(crate) fn new(workspace: &str, slug: &str, review_tag: Option<String>, fulls: Fulls) -> Result<Self> {
        Ok(Self { client: Client::new(workspace, slug)?, review_tag, fulls })
    }
}

impl Client {
    pub(crate) fn new(workspace: &str, slug: &str) -> Result<Self> {
        let token = token::read().map_err(|e| ActionFailure::Token { words: e.to_string() })?;
        let repo = client::Repo { workspace: workspace.into(), slug: slug.into(), header: token.header() };
        Ok(Self { repo, token })
    }

    fn operator(&self) -> Result<client::Repo> {
        let header = token::read_merge().map_err(|e| operator_failure(e.to_string()))?.header();
        Ok(client::Repo { header, ..self.repo.clone() })
    }
}

fn operator_failure(words: String) -> ActionFailure {
    ActionFailure::Failed { words: format!("The operator's merge token: {words}") }
}

fn operator_result(f: ActionFailure) -> ActionFailure {
    match f {
        ActionFailure::Token { words } => operator_failure(words),
        other => other,
    }
}

fn metadata(p: &Value) -> Pr {
    let text = |path: &str| p.pointer(path).and_then(Value::as_str).map(str::to_owned);
    let state = text("/state").unwrap_or_else(|| "?".into());
    Pr {
        id: p["id"].as_u64(),
        open: state == "OPEN",
        state,
        title: text("/title"),
        description: text("/description").unwrap_or_default(),
        branch: text("/source/branch/name"),
        dest: text("/destination/branch/name"),
    }
}

fn comment(value: &Value) -> Comment {
    let inline = value
        .get("inline")
        .filter(|v| !v.is_null())
        .map(|anchor| Anchor { path: anchor["path"].as_str().map(str::to_owned), line: anchor["to"].as_u64() });
    Comment {
        requested: None,
        id: value["id"].as_u64(),
        parent: value.pointer("/parent/id").and_then(Value::as_u64),
        body: value.pointer("/content/raw").and_then(Value::as_str).map(str::to_owned),
        inline,
    }
}

impl ForgeRead for Reader {
    fn identity(&self) -> std::result::Result<Identity, String> {
        match calls::identity(&self.client.repo, self.client.token.approver.as_deref()) {
            Ok(verdict) => Ok(Identity::Known(verdict)),
            Err(client::BitbucketError::Answered(a)) if a.status == 403 => Ok(Identity::Unknown),
            Err(e) => Err(e.to_string()),
        }
    }

    fn has_approver(&self) -> bool {
        self.client.token.approver.is_some()
    }

    fn open_prs(&self) -> Result<Vec<OpenPr>> {
        let mut prs = calls::open_prs(&self.client.repo, self.client.token.user.as_deref(), &self.fulls)?;
        for pr in &mut prs {
            pr.review_requested = self.review_tag.as_deref().is_some_and(|tag| pr.title.contains(tag));
        }
        Ok(prs)
    }

    fn pr(&self, id: u64) -> Result<Pr> {
        calls::pr_of(&self.client.repo, id).map(|p| metadata(&p))
    }

    fn branch_prs(&self, branch: &str) -> Result<Vec<Pr>> {
        calls::prs_of_branch(&self.client.repo, branch).map(|prs| prs.iter().map(metadata).collect())
    }

    fn observe(&self, id: u64) -> Result<PrSeen> {
        calls::read_pr(&self.client.repo, id, self.client.token.approver.as_deref().unwrap_or_default(), &self.fulls)
    }

    fn comments(&self, pr: u64, complete: bool) -> Result<Vec<Comment>> {
        let all = if complete {
            calls::thread_comments(&self.client.repo, pr)
        } else {
            calls::comments(&self.client.repo, pr)
        }?;
        Ok(all.iter().map(comment).collect())
    }

    fn threads(&self, pr: u64, ids: &[(u32, u64)]) -> Threads {
        let all = match calls::thread_comments(&self.client.repo, pr) {
            Ok(all) => all,
            Err(e) => return ids.iter().map(|(n, _)| (*n, Err(format!("{e:?}")))).collect(),
        };
        ids.iter()
            .map(|(n, id)| {
                let thread = calls::thread_root(&self.client.repo, pr, *id)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|root| parse::thread(&root, &all));
                (*n, thread)
            })
            .collect()
    }
}

impl ForgeWrite for Client {
    fn publish(&self, branch: &str, base: &str, text: (&str, &str), pr: Option<u64>) -> Result<u64> {
        let open = match pr {
            Some(n) => {
                let seen = calls::pr_of(&self.repo, n)?;
                if !calls::is_mine(&seen, branch, base) {
                    return Err(ActionFailure::Failed {
                        words: format!("PR {n} is no longer open from {branch} into {base}"),
                    });
                }
                Some(seen)
            }
            None => calls::open_of_branch(&self.repo, branch, base)?,
        };
        let mine = self.operator()?;
        match open {
            Some(seen) => calls::retitle(&mine, &seen, text),
            None => calls::open_pr(&mine, branch, base, text),
        }
        .map_err(operator_result)
    }

    fn comment(&self, pr: u64, post: crate::forge::CommentPost<'_>) -> Result<Comment> {
        calls::comment(&self.repo, pr, post.file, post.line, post.body)
    }

    fn reply(&self, pr: u64, parent: u64, body: &str, _check: &dyn Fn(&str) -> Result<()>) -> Result<Option<u64>> {
        calls::reply(&self.repo, pr, parent, body)
    }

    fn resolve(&self, pr: u64, comment: u64) -> Result<bool> {
        calls::resolve(&self.repo, pr, comment)
    }

    fn merge(&self, pr: u64, _head: &str) -> Result<String> {
        calls::merge(&self.operator()?, pr, false).map_err(operator_result)
    }
}
