//! The door's leak check (fac-1): credential values are masked before any text is kept or sent,
//! however short, escaped or cut at a bound.

use serde_json::Value;

/// What a secret becomes.
pub const MASK: &str = "‹secret›";

// A text still changing after this many decodings is withheld, never sent unchecked.
const DEPTH: usize = 16;
const WITHHELD: &str = "‹withheld: escaped too deeply to check›";

const TAIL: usize = 3;

const SECRET_KEYS: [&str; 7] = ["pass", "pwd", "secret", "token", "key", "auth", "credential"];
const PLAIN_KEYS: [&str; 4] = ["auth_database", "credentials_file", "credentials_key", "credentials_format"];

/// The values to mask, longest first.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    /// Adds a value, and its written forms in JSON and in MySQL's batch output.
    pub fn add(&mut self, value: &str) {
        let v = value.trim();
        if v.is_empty() {
            return;
        }
        let json = serde_json::to_string(v).unwrap_or_default();
        let json = json.trim_matches('"').to_owned();
        let tsv = v.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n");
        for form in [v.to_owned(), json, tsv] {
            if !self.values.contains(&form) {
                self.values.push(form);
            }
        }
        self.values.sort_by_key(|x| std::cmp::Reverse(x.len()));
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Every value it masks, each written form included.
    pub fn values(&self) -> &[String] {
        &self.values
    }

    /// A text with every secret masked.
    pub fn mask(&self, text: &str) -> String {
        let mut t = text.to_owned();
        for v in &self.values {
            if t.contains(v.as_str()) {
                t = t.replace(v.as_str(), MASK);
            }
        }
        t
    }

    /// A text cut at a bound, masked: a tail that begins a secret goes too.
    pub fn mask_cut(&self, text: &str) -> String {
        let mut t = self.mask(text);
        for v in &self.values {
            let start = t.char_indices().map(|(i, _)| i).find(|i| {
                let tail = &t[*i..];
                tail.len() >= TAIL.min(v.len()) && tail.len() < v.len() && v.starts_with(tail)
            });
            if let Some(i) = start {
                t.truncate(i);
                t.push_str(MASK);
            }
        }
        t
    }

    /// A JSON value with every string, key and number that is a secret masked.
    pub fn mask_value(&self, v: &Value) -> Value {
        match v {
            Value::String(s) => Value::String(self.mask_text(s, false)),
            Value::Number(n) if self.values.contains(&n.to_string()) => Value::String(MASK.into()),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.mask_value(x)).collect()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, x)| (self.mask_text(k, false), self.mask_value(x))).collect())
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => v.clone(),
        }
    }

    /// Masks a text as written and, where JSON escapes at any depth spell a secret, at every decoded
    /// depth. A text escaped deeper than can be checked is withheld.
    pub fn mask_text(&self, text: &str, cut: bool) -> String {
        let m = |t: &str| if cut { self.mask_cut(t) } else { self.mask(t) };
        let masked = m(text);
        let mut decoded = masked.clone();
        let mut hid = false;
        for _ in 0..DEPTH {
            let next = unescape_json(&decoded);
            if next == decoded {
                return if hid { decoded } else { masked };
            }
            decoded = m(&next);
            hid |= decoded != next;
        }
        WITHHELD.to_owned()
    }

    /// Masks JSON output (a document, or one per line) by its decoded values, so no escape spells a
    /// secret past it. A line that isn't JSON is masked with its escapes decoded.
    pub fn mask_json(&self, text: &str) -> String {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            return self.mask_value(&v).to_string();
        }
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let body = line.trim_end_matches('\n');
            match serde_json::from_str::<Value>(body) {
                Ok(v) => out.push_str(&self.mask_value(&v).to_string()),
                Err(_) => out.push_str(&self.mask_text(body, true)),
            }
            out.push_str(&line[body.len()..]);
        }
        out
    }

    /// Both sets of secrets.
    pub fn merged(mut self, other: &Self) -> Self {
        for v in &other.values {
            if !self.values.contains(v) {
                self.values.push(v.clone());
            }
        }
        self.values.sort_by_key(|x| std::cmp::Reverse(x.len()));
        self
    }
}

/// Whether a field's name says its value is a secret.
pub(crate) fn secret_key(k: &str) -> bool {
    let key = k.trim().trim_matches('"').to_lowercase();
    !PLAIN_KEYS.contains(&key.as_str()) && SECRET_KEYS.iter().any(|s| key.contains(s))
}

/// A URI's password, as written in it.
pub(crate) fn uri_password(v: &str) -> Option<String> {
    let rest = v.split_once("://")?.1;
    let auth = rest.split(['/', '?', '#']).next()?.rsplit_once('@')?.0;
    auth.split_once(':').map(|(_, p)| p.to_owned()).filter(|p| !p.is_empty())
}

/// JSON's string escapes decoded wherever they stand, surrogate pairs included.
pub fn unescape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                let Ok(hi) = u32::from_str_radix(&hex, 16) else {
                    out.push_str("\\u");
                    out.push_str(&hex);
                    continue;
                };
                out.push(surrogate(hi, &mut chars).unwrap_or('\u{fffd}'));
            }
            Some(e @ ('"' | '\\' | '/')) => out.push(e),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn surrogate(hi: u32, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    if !(0xD800..0xDC00).contains(&hi) {
        return char::from_u32(hi);
    }
    let rest: String = chars.clone().take(6).collect();
    let lo = rest.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok())?;
    for _ in 0..6 {
        chars.next();
    }
    char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
}

#[cfg(test)]
mod tests {
    use super::*;

    // fac-1: door secrets of any length, in every written form, at any cut.
    #[test]
    fn fac1_door_secrets_are_masked_in_every_form() {
        let mut r = Redactor::default();
        r.add("s3cr\\t\"x");
        r.add("hunter2");
        assert_eq!(r.mask("user door_ro on db.invalid"), "user door_ro on db.invalid");
        assert_eq!(r.mask("pw s3cr\\t\"x."), format!("pw {MASK}."));
        assert_eq!(r.mask("json s3cr\\\\t\\\"x"), format!("json {MASK}"));
        assert_eq!(r.mask("tsv s3cr\\\\t\"x"), format!("tsv {MASK}"));
        assert_eq!(r.mask("short hunter2"), format!("short {MASK}"));
        assert_eq!(r.mask_cut("cut at s3cr\\"), format!("cut at {MASK}"));
        let v = serde_json::json!({"a": ["x hunter2"], "hunter2": 1});
        assert_eq!(r.mask_value(&v), serde_json::json!({"a": [format!("x {MASK}")], MASK: 1}));
        let mut k = Redactor::default();
        k.add("pw");
        assert_eq!(k.mask("a pw"), format!("a {MASK}"), "no value is too short");
    }

    // fac-1 review (finding 2): a secret spelled with JSON escapes, even in a line cut before it
    // parses, is masked by its decoded value.
    #[test]
    fn fac1_json_escapes_spell_no_secret() {
        let mut r = Redactor::default();
        r.add("SENTINEL-key-99");
        let doc = r#"{"signature": "\u0053ENTINEL-key-99", "n": 1}"#;
        let masked = r.mask_json(doc);
        assert_eq!(serde_json::from_str::<Value>(&masked).unwrap()["signature"], MASK, "{masked}");
        let lines = format!("{doc}\n{{\"a\": \"x\"}}\n{{\"cut\": \"\\u0053ENTINEL-key");
        let masked = r.mask_json(&lines);
        assert!(!unescape_json(&masked).contains("SENTINEL-key"), "{masked}");
        assert_eq!(unescape_json(r#"\ud83d\ude00 \u0053"#), "\u{1f600} S");
    }

    fn decoded_fully(text: &str) -> String {
        let mut t = text.to_owned();
        loop {
            let next = unescape_json(&t);
            if next == t {
                return t;
            }
            t = next;
        }
    }

    // Review r2 finding 1, r3's: escape-spelled secrets in text are masked at every depth; text
    // hiding none stays as written; one escaped too deeply to check is withheld.
    #[test]
    fn fac1_escaped_secrets_in_text_are_masked() {
        let mut r = Redactor::default();
        r.add("SENTINEL-key-99");
        for text in [
            r#"error: {"key":"\u0053ENTINEL-key-99"}"#,
            r#"log: "{\"key\":\"\\u0053ENTINEL-key-99\"}""#,
            r#"first=\u0053ENTINEL-key-99; nested=\\u0053ENTINEL-key-99"#,
            r#"a=\\\\u0053ENTINEL-key-99 b=SENTINEL-key-99 c=\u0053\\u0045NTINEL-key-99"#,
        ] {
            let out = r.mask_text(text, true);
            assert!(!decoded_fully(&out).contains("SENTINEL"), "{text} -> {out}");
        }
        assert_eq!(r.mask_text(r#"a path C:\new\table"#, false), r#"a path C:\new\table"#);
        let deep = (0..20).fold("SENTINEL-key-99".to_owned(), |t, _| serde_json::to_string(&t).unwrap());
        assert_eq!(r.mask_text(&deep.replace('S', "\\u0053"), false), WITHHELD);
    }
}
