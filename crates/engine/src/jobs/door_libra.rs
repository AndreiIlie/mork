//! A Libra read (fac-86): its config, credential and token, comes on stdin never argv; over ssh it goes
//! through the forward, so Libra sees the whitelisted box's address, and checks Libra's own certificate.

use super::door_services::At;

/// `curl`, its own `.curlrc` never read, its config on stdin.
pub fn argv() -> Vec<String> {
    ["curl", "-q", "-K", "-"].iter().map(|s| (*s).to_owned()).collect()
}

// A value as curl's config reads it back: quoted, its backslashes and quotes escaped.
fn value(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

// HTTPS only, no redirect followed, quiet but for its errors; the status on a line of its own
// after the body; at the forward's socket when there is one.
fn common(url: &str, at: &At<'_>, seconds: u64) -> String {
    let mut c = format!(
        "url = {}\nproto = \"=https\"\ngloboff\nsilent\nshow-error\nmax-time = {seconds}\nwrite-out = \"\\n%{{http_code}}\"\n",
        value(url)
    );
    if let At::Socket(p) = at {
        c.push_str(&format!("unix-socket = {}\n", value(&p.display().to_string())));
    }
    c
}

/// The token request's config: the client's key and secret as its basic login, the grant in
/// its form body.
pub fn token(
    origin: &str,
    token_path: &str,
    scope: &str,
    key: &str,
    secret: &str,
    at: &At<'_>,
    seconds: u64,
) -> String {
    let mut c = common(&format!("{origin}{token_path}"), at, seconds);
    c.push_str(&format!("user = {}\n", value(&format!("{key}:{secret}"))));
    c.push_str(&format!("data = {}\n", value(&format!("grant_type=client_credentials&scope={scope}"))));
    c
}

/// The GET's config: the path at the origin, with the token.
pub fn get(origin: &str, path: &str, token: &str, at: &At<'_>, seconds: u64) -> String {
    let mut c = common(&format!("{origin}{path}"), at, seconds);
    c.push_str(&format!("header = {}\n", value(&format!("Authorization: Bearer {token}"))));
    c.push_str("header = \"Accept: application/json\"\n");
    c
}

/// A response's body and its HTTP status, from curl's output; no status when its output was cut
/// before it.
pub fn answer(out: &str) -> (&str, Option<u16>) {
    match out.rsplit_once('\n') {
        Some((body, code)) => match code.trim().parse() {
            Ok(c) => (body, Some(c)),
            Err(_) => (out, None),
        },
        None => (out, None),
    }
}

pub fn token_of(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("access_token")?.as_str().filter(|t| !t.is_empty()).map(str::to_owned)
}

/// Every token a token response holds, used or not: each is masked, whatever became of the call.
pub fn tokens_of(body: &str) -> Vec<String> {
    let Ok(serde_json::Value::Object(o)) = serde_json::from_str(body) else { return Vec::new() };
    let held = ["access_token", "refresh_token", "id_token"].iter().filter_map(|k| o.get(*k)?.as_str());
    held.filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// What a failed token request's answer said: Libra's `error` and `error_description` alone; a body
/// that isn't a whole JSON object (cut short, a proxy's page) could hold a token unread, so is withheld.
pub fn refusal(body: &str) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    let Ok(serde_json::Value::Object(o)) = serde_json::from_str(body) else {
        return "(its answer withheld: it can't be read whole)".into();
    };
    let said: serde_json::Map<String, serde_json::Value> =
        ["error", "error_description"].iter().filter_map(|k| Some(((*k).to_owned(), o.get(*k)?.clone()))).collect();
    serde_json::Value::Object(said).to_string()
}

/// An `https://host[:port]` origin's host and port.
pub fn address(origin: &str) -> Option<(&str, u16)> {
    let rest = origin.strip_prefix("https://")?;
    match rest.split_once(':') {
        Some((host, port)) => Some((host, port.parse().ok()?)),
        None => Some((rest, 443)),
    }
}
