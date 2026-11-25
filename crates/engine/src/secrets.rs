//! What must never leave (3.8, 3.10.6): a hit names the kind of secret and the place, never the
//! value.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use factory_types::config::Config;
use serde_json::Value;

use crate::paths::Home;

// Shorter values aren't masked (3.8.1).
const MIN: usize = 8;

/// The known secret values, each with its kind.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    known: Vec<(String, String)>,
}

fn json_values(text: &str, pointers: &[&str]) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    pointers.iter().filter_map(|p| v.pointer(p).and_then(Value::as_str).map(str::to_owned)).collect()
}

fn json_at(path: &Path, pointers: &[&str]) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    json_values(&text, pointers)
}

fn regular_beneath(dir: &Path, rel: &str) -> Option<String> {
    let mut text = String::new();
    regular_file_beneath(dir, rel)?.read_to_string(&mut text).ok()?;
    Some(text)
}

/// `rel` opened beneath `dir` if it is a regular file there: a seat writes the agent home, so no planted link leads out
/// and no FIFO holds the caller (fac-657).
pub fn regular_file_beneath(dir: &Path, rel: &str) -> Option<std::fs::File> {
    let file = factory_sys::open_beneath(dir, Path::new(rel)).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

// The whole text is always a value, and each `KEY=value` line's value as written: TOML reads `0xdeadbeef`
// as a number spelled otherwise (3.8.1, M4.4, M5.23).
fn values_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        strings_of(&v, &mut out);
    } else if let Ok(t) = toml::from_str::<toml::Value>(text) {
        strings_of(&serde_json::to_value(t).unwrap_or_default(), &mut out);
    }
    out.extend(
        text.lines()
            .filter_map(|l| l.trim().strip_prefix("export ").unwrap_or(l.trim()).split_once('='))
            .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_owned()),
    );
    out.push(text.trim().to_owned());
    out
}

fn strings_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        // A number is a value as written (`PIN=12345678` reads as TOML; M5.6).
        Value::Number(n) => out.push(n.to_string()),
        Value::Array(a) => a.iter().for_each(|x| strings_of(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings_of(x, out)),
        Value::Null | Value::Bool(_) => {}
    }
}

impl Secrets {
    /// Reads them: the token, the telemetry headers, the credential sets, the copy
    /// files, the agent home's logins (F-A1, F-A2).
    pub fn gather(home: &Home, config: &Config, telemetry: &BTreeMap<String, String>) -> Self {
        let mut s = Self::default();
        if let Ok(t) = crate::token::read() {
            s.add("the Bitbucket token", &t.token);
        }
        if let Ok(t) = crate::token::read_merge() {
            s.add("Andrei's Bitbucket token", &t.token);
        }
        if let Some(pat) = crate::azure::pat() {
            s.add("the Azure DevOps token", &pat);
            s.add("the Azure DevOps token", &crate::azure::header(&pat));
            s.add("the Azure DevOps token", crate::azure::header(&pat).trim_start_matches("Basic "));
        }
        for v in telemetry.values() {
            s.add("a telemetry header", v);
            for header in v.split(',') {
                if let Some((_, value)) = header.split_once('=') {
                    s.add("a telemetry header", value);
                }
            }
        }
        for seat in config.seats.values().filter_map(|x| x.credentials.clone()) {
            for file in files_under(Path::new(&seat), 0) {
                for v in std::fs::read_to_string(&file).map(|t| values_of(&t)).unwrap_or_default() {
                    s.add("a credential", &v);
                }
            }
        }
        for p in config.projects.values() {
            for f in &p.copy {
                for v in std::fs::read_to_string(Path::new(&p.repo).join(f)).map(|t| values_of(&t)).unwrap_or_default()
                {
                    s.add(&format!("a value of {f}"), &v);
                }
            }
        }
        s.add_agent_logins(&home.agent_home());
        if let Ok(key) = crate::web::key::read(&home.web_key()) {
            s.add("the web bridge key", &key);
        }
        s
    }

    fn add_agent_logins(&mut self, ah: &Path) {
        let claude = ["/claudeAiOauth/accessToken", "/claudeAiOauth/refreshToken"];
        for file in
            std::iter::once(ah.join(".claude/.credentials.json")).chain(files_under(&ah.join(".claude/accounts"), 0))
        {
            for v in json_at(&file, &claude) {
                self.add("the Claude login", &v);
            }
        }
        for v in json_at(&ah.join(".claude.json"), &["/primaryApiKey"]) {
            self.add("the Claude login", &v);
        }
        let codex = ["/tokens/id_token", "/tokens/access_token", "/tokens/refresh_token", "/OPENAI_API_KEY"];
        for v in json_at(&ah.join(".codex/auth.json"), &codex) {
            self.add("the Codex login", &v);
        }
        // Metadata fields like auth_method can match ordinary text; only OAuth token values are secrets (fac-679).
        let agy = [
            "/token/access_token",
            "/token/refresh_token",
            "/token/id_token",
            "/access_token",
            "/refresh_token",
            "/id_token",
        ];
        for v in regular_beneath(ah, crate::cli::agy::TOKEN).map(|t| json_values(&t, &agy)).unwrap_or_default() {
            self.add("the agy login", &v);
        }
    }

    pub(crate) fn add(&mut self, kind: &str, value: &str) {
        let v = value.trim();
        if v.chars().count() >= MIN && !self.known.iter().any(|(_, k)| k == v) {
            self.known.push((kind.to_owned(), v.to_owned()));
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.known.iter().map(|(_, v)| v.as_str())
    }

    /// The kind of the first secret a text holds.
    pub fn hit(&self, text: &str) -> Option<&str> {
        self.hit_value(text).map(|(k, _)| k)
    }

    /// The first secret a text holds: its kind and its value, which goes nowhere outside.
    pub fn hit_value(&self, text: &str) -> Option<(&str, &str)> {
        self.known.iter().find(|(_, v)| text.contains(v.as_str())).map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The text with every secret masked (`‹secret›`), for a notice or a log.
    pub fn mask(&self, text: &str) -> String {
        let mut t = text.to_owned();
        for (_, v) in &self.known {
            t = t.replace(v.as_str(), "‹secret›");
        }
        t
    }

    /// A JSON value with every string and key masked as written, before an encoding escapes a
    /// secret's quotes or backslashes past `mask`; a number that is a secret, masked as a string.
    pub fn mask_value(&self, v: &Value) -> Value {
        match v {
            Value::String(s) => Value::String(self.mask(s)),
            Value::Number(n) if self.known.iter().any(|(_, k)| *k == n.to_string()) => Value::String("‹secret›".into()),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.mask_value(x)).collect()),
            Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (self.mask(k), self.mask_value(x))).collect()),
            Value::Null | Value::Bool(_) | Value::Number(_) => v.clone(),
        }
    }
}

pub use factory_core::internals::internals;

// As its walls see it (3.7.3, M3.5): a symlink is never followed, in or out of the set; the depth
// is bounded.
fn files_under(dir: &Path, depth: u32) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if depth > 16 {
        return out;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            out.extend(files_under(&e.path(), depth + 1));
        } else if kind.is_file() {
            out.push(e.path());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    // why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    fn temp_home(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("factory-test-secrets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".gemini/antigravity-cli")).unwrap();
        dir
    }

    // fac-679: agy OAuth token metadata never triggers leak hits; real token secrets still do.
    #[test]
    fn fac679_agy_token_extracts_only_secret_fields() {
        let dir = temp_home("agy-fields");
        let token = dir.join(crate::cli::agy::TOKEN);
        let stored = json!({
            "token": {
                "access_token": "SENTINEL-agy-access-token-31",
                "token_type": "Bearer",
                "refresh_token": "SENTINEL-agy-refresh-token-29",
                "id_token": "SENTINEL-agy-id-token-37",
                "expiry": "2026-11-23T15:00:00.000000000+02:00",
                "expires_in": 3599,
            },
            "auth_method": "personal",
            "id_token_email": "agent@example.com",
        });
        std::fs::write(&token, stored.to_string()).unwrap();
        let mut s = Secrets::default();
        s.add_agent_logins(&dir);
        assert_eq!(s.hit("leak SENTINEL-agy-access-token-31"), Some("the agy login"));
        assert_eq!(s.hit("leak SENTINEL-agy-refresh-token-29"), Some("the agy login"));
        assert_eq!(s.hit("leak SENTINEL-agy-id-token-37"), Some("the agy login"));
        assert_eq!(s.hit("personal note for agent@example.com at 2026-11-23T15:00:00.000000000+02:00"), None);
        let flat = json!({"access_token": "SENTINEL-flat-access-41", "refresh_token": "SENTINEL-flat-refresh-43"});
        std::fs::write(&token, flat.to_string()).unwrap();
        let mut s = Secrets::default();
        s.add_agent_logins(&dir);
        assert_eq!(s.hit("SENTINEL-flat-access-41"), Some("the agy login"));
        assert_eq!(s.hit("SENTINEL-flat-refresh-43"), Some("the agy login"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // fac-657: a symlink planted in the agent home is ignored when reading agy's token.
    #[test]
    fn fac679_agy_token_ignores_symlink() {
        let dir = temp_home("agy-symlink");
        let outside = dir.join("outside.json");
        std::fs::write(&outside, json!({"token": {"access_token": "SENTINEL-linked-token-51"}}).to_string()).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join(crate::cli::agy::TOKEN)).unwrap();
        let mut s = Secrets::default();
        s.add_agent_logins(&dir);
        assert_eq!(s.hit("SENTINEL-linked-token-51"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
