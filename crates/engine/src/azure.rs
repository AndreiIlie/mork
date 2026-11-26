use std::time::Duration;

use crate::tickets::{Read, TicketsRead};
use mork_types::work::Ticket;
use serde_json::Value;

/// The token, as the file holds it.
pub(crate) fn pat(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

/// The `Authorization` header's value for a token: basic with an empty user.
pub fn header(pat: &str) -> String {
    format!("Basic {}", crate::encoding::base64(format!(":{pat}").as_bytes()))
}

pub(crate) struct Client {
    agent: ureq::Agent,
    base: String,
    header: String,
}

impl Client {
    pub(crate) fn new(configured: &str, token: &str) -> Option<Self> {
        Some(Self { agent: agent(), base: configured.to_owned(), header: header(&pat(token)?) })
    }
}

fn agent() -> ureq::Agent {
    // No redirect is followed: the header goes to the one origin, and a sign-in page is no answer.
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

impl TicketsRead for Client {
    fn get(&self, id: u64) -> Read {
        let base = &self.base;
        let url = format!("{base}/{id}?api-version=7.1");
        let Ok(mut resp) = self.agent.get(&url).header("Authorization", &self.header).call() else {
            return Read::Unavailable;
        };
        match resp.status().as_u16() {
            400 | 403 | 404 | 410 => return Read::Missing,
            200 => {}
            _ => return Read::Unavailable,
        }
        let body = resp.body_mut().read_to_string().ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        body.and_then(|v| ticket(id, &v)).map_or(Read::Unavailable, Read::Ticket)
    }
}

fn ticket(id: u64, v: &Value) -> Option<Ticket> {
    let fields = v.get("fields")?;
    let field = |name: &str| fields.get(name).and_then(Value::as_str).unwrap_or_default();
    Some(Ticket {
        id,
        // Plain text, not rich text: a `<` in a title is the title's own.
        title: field("System.Title").to_owned(),
        description: plain(field("System.Description")),
        repro_steps: plain(field("Microsoft.VSTS.TCM.ReproSteps")),
    })
}

fn plain(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim_start_matches('/').to_lowercase();
        let name = tag.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or_default();
        if matches!(name, "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4") {
            text.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let joined = lines.join("\n");
    joined.trim().to_owned()
}
