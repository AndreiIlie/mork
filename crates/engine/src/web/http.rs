//! HTTP/1.1 request head parsing, auth/Origin/upgrade checks, and small responses (fac-671).

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use percent_encoding::percent_decode_str;

use super::key;

/// Maximum bytes read for a single HTTP request head.
pub const MAX_HEAD: usize = 16 * 1024;
/// Total deadline for reading an HTTP request head.
pub const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Parsed HTTP/1.x request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub version: u8,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// Returns the first header value matching `name` ASCII case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Whether any header named `name` contains `token` as a comma-separated ASCII token.
    pub fn has_token(&self, name: &str, token: &str) -> bool {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v.split(','))
            .any(|part| part.trim().eq_ignore_ascii_case(token))
    }
}

/// Outcome of validating a `/ws` WebSocket upgrade request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsCheck {
    Ok { ws_key: String },
    Status(u16, &'static str),
}

/// Reads an HTTP request head from `stream`, returning it and any bytes buffered past `\r\n\r\n`.
pub fn read_head(stream: &mut TcpStream) -> io::Result<Option<(Head, Vec<u8>)>> {
    let start = crate::clock::instant();
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        let elapsed = crate::clock::elapsed(start);
        if elapsed >= HEAD_TIMEOUT {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "request head timed out"));
        }
        let _ = stream.set_read_timeout(Some(HEAD_TIMEOUT.saturating_sub(elapsed)));
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return if buf.is_empty() { Ok(None) } else { Err(io::Error::from(io::ErrorKind::UnexpectedEof)) };
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = find_head_end(&buf) {
            if end > MAX_HEAD {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "request head too large"));
            }
            break end;
        }
        if buf.len() >= MAX_HEAD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "request head too large"));
        }
    };
    let head = parse_head(&buf[..head_end]).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad HTTP"))?;
    Ok(Some((head, buf[head_end..].to_vec())))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Parses raw HTTP/1.x head bytes ending in `\r\n\r\n`.
pub fn parse_head(bytes: &[u8]) -> Option<Head> {
    let mut raw = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut raw);
    match req.parse(bytes) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return None,
    }
    let method = req.method?.to_owned();
    let version = req.version?;
    let target = req.path?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
        None => (target.to_owned(), None),
    };
    let mut headers = Vec::with_capacity(req.headers.len());
    for h in req.headers {
        let value = std::str::from_utf8(h.value).ok()?.trim().to_owned();
        headers.push((h.name.to_owned(), value));
    }
    Some(Head { method, path, query, version, headers })
}

/// Extracts the bridge key from the `key` query parameter or `Authorization: Bearer <key>`.
pub fn extract_key(head: &Head) -> Option<String> {
    if let Some(q) = &head.query {
        for pair in q.split('&') {
            if let Some(val) = pair.strip_prefix("key=") {
                return percent_decode_str(val).decode_utf8().ok().map(std::borrow::Cow::into_owned);
            }
        }
    }
    let auth = head.header("authorization")?;
    let (scheme, token) = auth.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim().to_owned()).filter(|s| !s.is_empty())
}

/// Constant-time check of the request key against `expected`.
pub fn key_matches(head: &Head, expected: &str) -> bool {
    extract_key(head).is_some_and(|got| key::constant_time_eq(got.as_bytes(), expected.as_bytes()))
}

/// Constant-time check of the `Authorization: Bearer <key>` header alone: a key in a URL never opens an image (fac-696).
pub fn bearer_matches(head: &Head, expected: &str) -> bool {
    let token =
        head.header("authorization").and_then(|a| a.split_once(' ')).filter(|(s, _)| s.eq_ignore_ascii_case("bearer"));
    token.is_some_and(|(_, t)| key::constant_time_eq(t.trim().as_bytes(), expected.as_bytes()))
}

/// Whether an `Origin` header is absent or `http://` on `localhost`, `127.0.0.1`, or `[::1]` (any port).
pub fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(raw) = origin else { return true };
    let rest = match raw.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("http://") => &raw[7..],
        _ => return false,
    };
    if rest.is_empty() || rest.bytes().any(|b| matches!(b, b'/' | b'?' | b'#' | b'@' | b' ' | b'\t')) {
        return false;
    }
    if let Some(after) = rest.strip_prefix("[::1]") {
        return valid_optional_port(after);
    }
    let (host, port) = match rest.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    let host_ok = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    host_ok && port.is_none_or(valid_port)
}

fn valid_optional_port(after: &str) -> bool {
    match after {
        "" => true,
        s => s.strip_prefix(':').is_some_and(valid_port),
    }
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

/// Validates a `/ws` request in spec order: key (401), Origin (403), method (405), upgrade headers (426).
pub fn check_ws_upgrade(head: &Head, expected_key: &str) -> WsCheck {
    if !key_matches(head, expected_key) {
        return WsCheck::Status(401, "Unauthorized");
    }
    if !origin_allowed(head.header("origin")) {
        return WsCheck::Status(403, "Forbidden");
    }
    if head.method != "GET" {
        return WsCheck::Status(405, "Method Not Allowed");
    }
    let upgrade_ok = head.version == 1
        && head.has_token("upgrade", "websocket")
        && head.header("sec-websocket-version").is_some_and(|v| v.trim() == "13");
    let ws_key = head.header("sec-websocket-key").map(str::trim).filter(|k| !k.is_empty());
    match (upgrade_ok, ws_key) {
        (true, Some(k)) => WsCheck::Ok { ws_key: k.to_owned() },
        _ => WsCheck::Status(426, "Upgrade Required"),
    }
}

/// Writes an empty HTTP/1.1 status response with `Connection: close` in a single write.
pub fn write_status(w: &mut impl Write, code: u16, reason: &str) -> io::Result<()> {
    let head = format!("HTTP/1.1 {code} {reason}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    w.write_all(head.as_bytes())?;
    w.flush()
}

/// Writes the HTTP/1.1 `101 Switching Protocols` WebSocket handshake response in a single write.
pub fn write_101(w: &mut impl Write, accept: &str) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    w.write_all(head.as_bytes())?;
    w.flush()
}

/// Writes an HTTP/1.1 `200 OK` static file response (headers only when `head_only`).
pub fn write_file(w: &mut impl Write, content_type: &str, file: File, head_only: bool) -> io::Result<()> {
    let len = file.metadata()?.len();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
    );
    w.write_all(head.as_bytes())?;
    if !head_only {
        io::copy(&mut file.take(len), w)?;
    }
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn head(raw: &str) -> Head {
        parse_head(raw.as_bytes()).unwrap()
    }

    #[test]
    fn seeded_request_heads_preserve_fields_and_require_a_complete_head() {
        for seed in 0..128 {
            eprintln!("HTTP seed {seed}");
            let mut rng = fastrand::Rng::with_seed(seed);
            let method = rng.choice(["GET", "HEAD", "POST"]).unwrap();
            let path = format!("/items/{}", rng.u32(..));
            let query = rng.bool().then(|| format!("page={}&sort=name", rng.u16(..)));
            let target = query.as_ref().map_or_else(|| path.clone(), |q| format!("{path}?{q}"));
            let version = rng.u8(0..=1);
            let value = format!("test-{}", rng.u64(..));
            let prefix = format!("{method} {target} HTTP/1.{version}\r\nX-Item: ");
            let wire = format!("{prefix}{value}\r\n\r\n");
            let parsed = parse_head(wire.as_bytes()).unwrap();
            assert_eq!((parsed.method.as_str(), parsed.version), (method, version), "seed {seed}");
            assert_eq!((&parsed.path, &parsed.query), (&path, &query), "seed {seed}");
            assert_eq!(parsed.header("x-item"), Some(value.as_str()), "seed {seed}");
            let cut = rng.usize(..wire.len());
            assert!(parse_head(&wire.as_bytes()[..cut]).is_none(), "seed {seed}, cut {cut}");

            let mut bytes = vec![0; rng.usize(1..=64)];
            rng.fill(&mut bytes);
            let _ = parse_head(&bytes);
            let mut malformed = prefix.into_bytes();
            malformed.push(0);
            malformed.extend(bytes);
            malformed.extend(b"\r\n\r\n");
            assert!(parse_head(&malformed).is_none(), "seed {seed}");
        }
    }

    #[test]
    fn key_check_covers_missing_wrong_and_right_in_query_and_header() {
        let missing = head("GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(!key_matches(&missing, KEY));
        let wrong_q = head("GET /ws?key=deadbeef HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert!(!key_matches(&wrong_q, KEY));
        let right_q = head(&format!("GET /ws?foo=1&key={KEY}&bar=2 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"));
        assert!(key_matches(&right_q, KEY));
        let wrong_h = head("GET /ws HTTP/1.1\r\nAuthorization: Bearer bad\r\n\r\n");
        assert!(!key_matches(&wrong_h, KEY));
        let right_h = head(&format!("GET /ws HTTP/1.1\r\nAuthorization: Bearer {KEY}\r\n\r\n"));
        assert!(key_matches(&right_h, KEY));
    }

    #[test]
    fn origin_rule_allows_localhost_http_and_rejects_others() {
        for ok in [
            None,
            Some("http://localhost"),
            Some("http://localhost:5173"),
            Some("http://127.0.0.1"),
            Some("http://127.0.0.1:41000"),
            Some("http://[::1]"),
            Some("http://[::1]:5173"),
            Some("HTTP://LOCALHOST:8080"),
        ] {
            assert!(origin_allowed(ok), "expected allowed: {ok:?}");
        }
        for bad in [
            "https://localhost:5173",
            "https://127.0.0.1:41000",
            "http://example.com",
            "http://localhost.evil.com",
            "http://127.0.0.1.evil.com",
            "http://localhost:5173/path",
            "http://user@localhost:5173",
            "http://localhost:99999",
            "http://[::1]evil",
            "null",
            "",
        ] {
            assert!(!origin_allowed(Some(bad)), "expected rejected: {bad:?}");
        }
    }

    #[test]
    fn ws_upgrade_enforces_order_and_returns_426_without_upgrade_headers() {
        let no_key = head("GET /ws HTTP/1.1\r\nOrigin: https://evil.com\r\n\r\n");
        assert_eq!(check_ws_upgrade(&no_key, KEY), WsCheck::Status(401, "Unauthorized"));

        let bad_origin = head(&format!("GET /ws?key={KEY} HTTP/1.1\r\nOrigin: https://evil.com\r\n\r\n"));
        assert_eq!(check_ws_upgrade(&bad_origin, KEY), WsCheck::Status(403, "Forbidden"));

        let post = head(&format!("POST /ws?key={KEY} HTTP/1.1\r\nOrigin: http://localhost:5173\r\n\r\n"));
        assert_eq!(check_ws_upgrade(&post, KEY), WsCheck::Status(405, "Method Not Allowed"));

        let plain_get = head(&format!("GET /ws?key={KEY} HTTP/1.1\r\nOrigin: http://localhost:5173\r\n\r\n"));
        assert_eq!(check_ws_upgrade(&plain_get, KEY), WsCheck::Status(426, "Upgrade Required"));

        let good = head(&format!(
            "GET /ws?key={KEY} HTTP/1.1\r\nOrigin: http://localhost:5173\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ));
        assert_eq!(check_ws_upgrade(&good, KEY), WsCheck::Ok { ws_key: "dGhlIHNhbXBsZSBub25jZQ==".into() });
    }
}
