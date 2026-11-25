//! The HTTP calls to Bitbucket's API (F-B1–F-B12), with the token's header. Every answer comes
//! back as status, body and `Location`; what they mean is `parse`'s and the caller's.

use std::time::Duration;

use serde_json::Value;

/// One repository's API.
#[derive(Debug, Clone)]
pub struct Repo {
    /// `https://api.bitbucket.org/2.0`, or the test's fake (1.10.6).
    pub api: String,
    pub workspace: String,
    pub slug: String,
    /// The `Authorization` header's value.
    pub header: String,
}

/// An answer: status, body (JSON, or a string), `Location`.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: Value,
    pub location: Option<String>,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Bitbucket's own words for a refusal.
    pub fn words(&self) -> String {
        self.body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.body.as_str().map(str::to_owned))
            .unwrap_or_else(|| self.body.to_string())
    }
}

/// A call that got no usable answer: Bitbucket unreachable, or an answer of refusal.
#[derive(Debug)]
pub enum BitbucketError {
    Unreachable(Box<ureq::Error>),
    /// A status outside 2xx, with Bitbucket's body.
    Answered(Answer),
}

impl std::fmt::Display for BitbucketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(e) => write!(f, "Bitbucket couldn't be reached: {e}"),
            Self::Answered(a) => write!(f, "Bitbucket answered {}: {}", a.status, a.words()),
        }
    }
}

impl std::error::Error for BitbucketError {}

/// The API's root: the fake's under the test settings, else Bitbucket's.
pub fn api() -> String {
    crate::faults::test_env("FACTORY_TEST_BITBUCKET").unwrap_or_else(|| "https://api.bitbucket.org/2.0".into())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

fn answer(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Answer, BitbucketError> {
    let mut resp = r.map_err(|e| BitbucketError::Unreachable(Box::new(e)))?;
    let status = resp.status().as_u16();
    let location = resp.headers().get("Location").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok(Answer { status, body, location })
}

impl Repo {
    fn url(&self, path: &str) -> String {
        format!("{}/repositories/{}/{}{path}", self.api, self.workspace, self.slug)
    }

    /// A GET of a path under the repository, or of a whole URL (a `next` link, a task).
    pub fn get(&self, path_or_url: &str) -> Result<Answer, BitbucketError> {
        let url = if path_or_url.starts_with("http") { path_or_url.to_owned() } else { self.url(path_or_url) };
        answer(agent().get(&url).header("Authorization", &self.header).call())
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Answer, BitbucketError> {
        answer(
            agent()
                .post(&self.url(path))
                .header("Authorization", &self.header)
                .header("Content-Type", "application/json")
                .send(body.to_string()),
        )
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<Answer, BitbucketError> {
        answer(
            agent()
                .put(&self.url(path))
                .header("Authorization", &self.header)
                .header("Content-Type", "application/json")
                .send(body.to_string()),
        )
    }

    /// `GET /user`: the account the token acts as (F-B12).
    pub fn user(&self) -> Result<Answer, BitbucketError> {
        answer(agent().get(&format!("{}/user", self.api)).header("Authorization", &self.header).call())
    }

    /// Every page of a list, following `next` (at most 20 pages).
    pub fn all(&self, first: &str) -> Result<Vec<Value>, BitbucketError> {
        let mut out = Vec::new();
        let mut next = Some(first.to_owned());
        let mut pages = 0;
        while let Some(url) = next.take() {
            let a = self.get(&url)?;
            if !a.ok() {
                return Err(BitbucketError::Answered(a));
            }
            out.extend(a.body.get("values").and_then(Value::as_array).cloned().unwrap_or_default());
            pages += 1;
            next = a.body.get("next").and_then(Value::as_str).map(str::to_owned).filter(|_| pages < 20);
        }
        Ok(out)
    }
}
