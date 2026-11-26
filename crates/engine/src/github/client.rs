use super::Client;
use crate::forge::Result;
use mork_types::work::ActionFailure;
use serde_json::Value;

pub(super) const API: &str = "https://api.github.com/";

pub(super) fn failed(words: impl Into<String>) -> ActionFailure {
    ActionFailure::Failed { words: words.into() }
}

pub(super) struct Answer {
    pub status: u16,
    pub body: Value,
    next: Option<String>,
}

impl Answer {
    pub fn value(self) -> Result<Value> {
        if (200..300).contains(&self.status) {
            return Ok(self.body);
        }
        let msg = self.body["message"].as_str().unwrap_or("request refused");
        let words = format!("GitHub answered {}: {msg}", self.status);
        Err(match self.status {
            401 => ActionFailure::Token { words },
            404 => ActionFailure::Gone { words },
            _ => failed(words),
        })
    }
}

impl Client {
    pub(super) fn mask(&self, words: &str) -> String {
        let basic = super::git_header(&self.token);
        words.replace(&self.token, "‹secret›").replace(basic.trim_start_matches("Basic "), "‹secret›")
    }

    pub fn repo_url(&self, path: &str) -> String {
        format!("{API}repos/{}/{}{path}", self.owner, self.repo)
    }

    pub(super) fn send(&self, method: &str, url: &str, body: Option<&Value>) -> Result<Answer> {
        if !url.starts_with(API) {
            return Err(failed("Unexpected GitHub API origin"));
        }
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "pull-request-client")
            .header("Accept", "application/vnd.github+json")
            .header("Content-Type", "application/json")
            .body(body.map(Value::to_string).unwrap_or_default())
            .map_err(|_| failed("Invalid GitHub request"))?;
        let mut response = self.agent.run(request).map_err(|_| failed("GitHub could not be reached"))?;
        let status = response.status().as_u16();
        let next = response.headers().get("link").and_then(|v| v.to_str().ok()).and_then(|s| {
            s.split(',').find_map(|link| {
                let (url, rel) = link.trim().split_once(';')?;
                (rel.trim() == "rel=\"next\"").then(|| url.trim().trim_matches(['<', '>']).to_owned())
            })
        });
        let bytes = response.body_mut().read_to_string().map_err(|_| failed("Unreadable GitHub response"))?;
        let mut parsed: Value = serde_json::from_str(&bytes).unwrap_or(Value::Null);
        if status >= 400 {
            let words = self.mask(parsed["message"].as_str().unwrap_or("request refused"));
            parsed = serde_json::json!({"message":words});
        }
        Ok(Answer { status, body: parsed, next })
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", &self.repo_url(path), None)?.value()
    }

    pub fn all(&self, path: &str) -> Result<Vec<Value>> {
        let mut url = Some(self.repo_url(path));
        let mut rows = Vec::new();
        while let Some(page) = url.take() {
            let mut answer = self.send("GET", &page, None)?;
            url = answer.next.take();
            let body = answer.value()?;
            let Value::Array(mut values) = body else {
                return Err(failed("Expected a GitHub list"));
            };
            rows.append(&mut values);
        }
        Ok(rows)
    }

    pub fn login(&self) -> Result<String> {
        if let Some(login) = self.login.get() {
            return Ok(login.clone());
        }
        let user = self.send("GET", &format!("{API}user"), None)?.value()?;
        let login = super::parse::string(&user, "/login")?.to_owned();
        let _ = self.login.set(login.clone());
        Ok(login)
    }
}
