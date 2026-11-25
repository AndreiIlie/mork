//! Azure DevOps (fac-292): the work items a PR refers to, read with the personal access token of
//! `~/.config/azure-devops/pat`. Only this module holds it: no seat, prompt or log does.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use factory_types::config::ProjectConfig;
use factory_types::work::{OpenPr, Ticket};
use serde_json::Value;

fn api(configured: &str) -> String {
    crate::faults::test_env("FACTORY_TEST_AZURE").unwrap_or_else(|| configured.to_owned())
}

/// The token, as the file holds it.
pub fn pat() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(".config/azure-devops/pat")).ok()?;
    Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

/// The `Authorization` header's value for a token: basic with an empty user.
pub fn header(pat: &str) -> String {
    format!("Basic {}", crate::token::base64(format!(":{pat}").as_bytes()))
}

enum Read {
    Ticket(Ticket),
    Missing,
    // The rest of this poll's reads wait for the next: Azure is down or refuses the token.
    Unavailable,
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

fn read(agent: &ureq::Agent, base: &str, header: &str, id: u64) -> Read {
    let url = format!("{base}/{id}?api-version=7.1");
    let Ok(mut resp) = agent.get(&url).header("Authorization", header).call() else { return Read::Unavailable };
    match resp.status().as_u16() {
        400 | 403 | 404 | 410 => return Read::Missing,
        200 => {}
        _ => return Read::Unavailable,
    }
    let body = resp.body_mut().read_to_string().ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    body.and_then(|v| ticket(id, &v)).map_or(Read::Unavailable, Read::Ticket)
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

type Tickets = (String, Vec<Ticket>);

/// What each open PR's tickets were last read for: its head and the numbers it referred to.
#[derive(Debug, Default, Clone)]
pub struct Cache(Arc<Mutex<BTreeMap<(String, u64), Tickets>>>);

impl Cache {
    /// The PRs with the tickets they refer to, read again for a new head or new numbers; a PR whose
    /// tickets couldn't be read has none until a later poll reads them. Read only for projects with `tickets`.
    pub fn attach(&self, project: &str, config: &ProjectConfig, mut prs: Vec<OpenPr>) -> Vec<OpenPr> {
        let Some(tickets_cfg) = &config.tickets else { return prs };
        let Some(pat) = pat() else { return prs };
        let base = api(&tickets_cfg.api);
        let (agent, header) = (agent(), header(&pat));
        let Ok(mut seen) = self.0.lock() else { return prs };
        seen.retain(|(p, n), _| p != project || prs.iter().any(|x| x.pr == *n));
        let mut down = false;
        for pr in &mut prs {
            let numbers = factory_core::reviews::tickets::candidates(&pr.title, &pr.description, &pr.branch);
            let version = format!("{} {numbers:?}", pr.head);
            let key = (project.to_owned(), pr.pr);
            if let Some((_, tickets)) = seen.get(&key).filter(|(v, _)| *v == version) {
                pr.tickets.clone_from(tickets);
                continue;
            }
            let mut tickets = Vec::new();
            let mut read_all = true;
            for n in numbers {
                // Azure being down for this poll, no other PR is tried: each call may wait out its timeout.
                if down {
                    read_all = false;
                    break;
                }
                match read(&agent, &base, &header, n) {
                    Read::Ticket(t) => tickets.push(t),
                    Read::Missing => {}
                    Read::Unavailable => {
                        down = true;
                        read_all = false;
                        break;
                    }
                }
            }
            if read_all {
                seen.insert(key, (version, tickets.clone()));
                pr.tickets = tickets;
            }
        }
        prs
    }
}
