mod client;
mod comments;
mod parse;
#[cfg(test)]
mod tests;
mod threads;
mod token;

use crate::forge::{Comment, CommentPost, ForgeRead, ForgeWrite, Identity, Pr, Result};
use client::failed;
use mork_types::config::ForgeConfig;
use mork_types::review::Threads;
use mork_types::work::{ActionFailure, OpenPr, PrSeen, PrState};
use serde_json::{Value, json};
pub(crate) use token::{git_header, pinned, read_file, source};

pub(crate) struct Client {
    agent: ureq::Agent,
    owner: String,
    repo: String,
    token: String,
    approver: Option<String>,
    review_tag: Option<String>,
    login: std::cell::OnceCell<String>,
}

impl Client {
    pub fn new(config: &ForgeConfig) -> Result<Self> {
        let ForgeConfig::Github { owner, repo, token, approver, review_tag, .. } = config else {
            return Err(failed("Expected a GitHub forge"));
        };
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(std::time::Duration::from_secs(60)))
            .build()
            .into();
        Ok(Self {
            agent,
            owner: owner.clone(),
            repo: repo.clone(),
            token: token::read(token)?,
            approver: approver.clone().filter(|_| config.merges_on_approval()),
            review_tag: review_tag.clone(),
            login: Default::default(),
        })
    }

    fn branch_rows(&self, branch: &str, state: &str) -> Result<Vec<Value>> {
        let head = format!("{}:{branch}", self.owner);
        let encoded = percent_encoding::utf8_percent_encode(&head, percent_encoding::NON_ALPHANUMERIC);
        let rows =
            self.all(&format!("/pulls?head={encoded}&state={state}&sort=created&direction=desc&per_page=100"))?;
        Ok(rows
            .into_iter()
            .filter(|p| parse::same_repo(p, &self.owner, &self.repo) && p["head"]["ref"] == branch)
            .collect())
    }
}

impl ForgeRead for Client {
    fn identity(&self) -> std::result::Result<Identity, String> {
        match self.login() {
            Ok(me) => Ok(Identity::Known(
                self.approver
                    .as_ref()
                    .filter(|login| me.eq_ignore_ascii_case(login))
                    .map(|_| "GitHub approval merges require a token from another account than the approver".into()),
            )),
            Err(ActionFailure::Token { words }) => Ok(Identity::Known(Some(words))),
            Err(e) => Err(format!("{e:?}")),
        }
    }

    fn has_approver(&self) -> bool {
        self.approver.is_some()
    }

    fn open_prs(&self) -> Result<Vec<OpenPr>> {
        let login = self.login()?;
        self.all("/pulls?state=open&per_page=100")?
            .iter()
            .map(|p| parse::open(p, &login, self.review_tag.as_deref(), !parse::same_repo(p, &self.owner, &self.repo)))
            .collect()
    }

    fn pr(&self, id: u64) -> Result<Pr> {
        parse::metadata(&self.get(&format!("/pulls/{id}"))?)
    }

    fn branch_prs(&self, branch: &str) -> Result<Vec<Pr>> {
        self.branch_rows(branch, "all")?.iter().map(parse::metadata).collect()
    }

    fn observe(&self, id: u64) -> Result<PrSeen> {
        let p = self.get(&format!("/pulls/{id}"))?;
        let mut seen = parse::seen(&p)?;
        if let Some(approver) = self.approver.as_deref()
            && seen.state == PrState::Open
            && parse::same_repo(&p, &self.owner, &self.repo)
        {
            let reviews = self.all(&format!("/pulls/{id}/reviews?per_page=100"))?;
            seen.approved = parse::approved(&reviews, approver, &seen.head);
        }
        Ok(seen)
    }

    fn comments(&self, pr: u64, _complete: bool) -> Result<Vec<Comment>> {
        comments::list(self, pr)
    }

    fn threads(&self, pr: u64, ids: &[(u32, u64)]) -> Threads {
        threads::read(self, pr, ids)
    }
}

impl ForgeWrite for Client {
    fn publish(&self, branch: &str, base: &str, (title, body): (&str, &str), pr: Option<u64>) -> Result<u64> {
        let open = match pr {
            Some(n) => {
                let p = self.get(&format!("/pulls/{n}"))?;
                if p["state"] != "open"
                    || p["head"]["ref"] != branch
                    || p["base"]["ref"] != base
                    || !parse::same_repo(&p, &self.owner, &self.repo)
                {
                    return Err(failed(format!("PR {n} is no longer open from {branch} into {base}")));
                }
                Some(n)
            }
            None => self
                .branch_rows(branch, "open")?
                .iter()
                .find(|p| p["base"]["ref"] == base)
                .map(parse::number)
                .transpose()?,
        };
        let response = match open {
            Some(n) => self.send(
                "PATCH",
                &self.repo_url(&format!("/pulls/{n}")),
                Some(&json!({ "title":title, "body":body })),
            )?,
            None => self.send(
                "POST",
                &self.repo_url("/pulls"),
                Some(&json!({ "head":branch,"base":base,"title":title,"body":body })),
            )?,
        };
        parse::number(&response.value()?)
    }

    fn comment(&self, pr: u64, post: CommentPost<'_>) -> Result<Comment> {
        comments::post(self, pr, post)
    }

    fn reply(&self, pr: u64, parent: u64, body: &str, check: &dyn Fn(&str) -> Result<()>) -> Result<Option<u64>> {
        comments::reply(self, pr, parent, body, check).map(Some)
    }

    fn resolve(&self, pr: u64, id: u64) -> Result<bool> {
        threads::resolve(self, pr, id)
    }

    fn merge(&self, pr: u64, head: &str) -> Result<String> {
        let answer = self.send(
            "PUT",
            &self.repo_url(&format!("/pulls/{pr}/merge")),
            Some(&json!({"merge_method":"merge","sha":head})),
        )?;
        if answer.status == 409 {
            let seen = self.observe(pr)?;
            return Err(ActionFailure::Guard {
                words: format!("GitHub refused the approved head {head}. PR {pr} is at {}", seen.head),
                pr: Some(Box::new(seen)),
            });
        }
        if answer.status == 405 && self.get(&format!("/pulls/{pr}"))?["mergeable"] == false {
            return Err(ActionFailure::Conflict {
                words: "GitHub reports merge conflicts".into(),
                tip: None,
                files: vec![],
            });
        }
        let result = answer.value()?;
        if result["merged"] != true {
            return Err(failed("GitHub did not merge the pull request"));
        }
        Ok(parse::string(&result, "/sha")?.into())
    }
}
