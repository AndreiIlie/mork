//! The Bitbucket token file (F-B0): `token=…`, then optional `user=…` and `approver=…`.

/// What is wrong with the token file.
#[derive(Debug)]
pub enum TokenError {
    NoHome,
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    /// A line that isn't `key=value`, or a key F-B0 doesn't have.
    Line {
        line: usize,
        what: String,
    },
    NoToken,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => write!(f, "no HOME"),
            Self::Read { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Line { line, what } => write!(f, "line {line}: {what}"),
            Self::NoToken => write!(f, "the token file has no `token=`"),
        }
    }
}

impl std::error::Error for TokenError {}

impl From<TokenError> for String {
    fn from(e: TokenError) -> Self {
        e.to_string()
    }
}

/// The token file's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub token: String,
    pub user: Option<String>,
    pub approver: Option<String>,
}

/// Reads the token file.
pub fn parse(text: &str) -> Result<Token, TokenError> {
    let mut t = Token { token: String::new(), user: None, approver: None };
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(TokenError::Line { line: n + 1, what: "not `key=value`".into() });
        };
        let v = v.trim().to_owned();
        match k.trim() {
            "token" => t.token = v,
            "user" => t.user = Some(v).filter(|v| !v.is_empty()),
            "approver" => t.approver = Some(v).filter(|v| !v.is_empty()),
            other => return Err(TokenError::Line { line: n + 1, what: format!("unknown key `{other}`") }),
        }
    }
    if t.token.is_empty() {
        return Err(TokenError::NoToken);
    }
    Ok(t)
}

impl Token {
    /// The `Authorization` header's value: basic with a `user`, else bearer (F-B0).
    pub fn header(&self) -> String {
        match &self.user {
            Some(u) => format!("Basic {}", base64(format!("{u}:{}", self.token).as_bytes())),
            None => format!("Bearer {}", self.token),
        }
    }

    /// The header git gets (3.7.1): Bitbucket's git refuses `Bearer`, so an access token (no
    /// `user`) goes as `Basic` with user `x-token-auth`; with a `user`, as the API's (M5.32).
    pub fn git_header(&self) -> String {
        let user = self.user.as_deref().unwrap_or("x-token-auth");
        format!("Basic {}", base64(format!("{user}:{}", self.token).as_bytes()))
    }
}

/// Reads `~/.config/factory/bitbucket-token` (1.3).
pub fn read() -> Result<Token, TokenError> {
    parse(&read_text()?)
}

std::thread_local! {
    // The token file's text as an action's guard read it: the call on this thread uses that
    // text, never the file read again (M5.1).
    static PINNED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with the token file read as `text` on this thread.
pub fn pinned<T>(text: String, f: impl FnOnce() -> T) -> T {
    let before = PINNED.with(|p| p.borrow_mut().replace(text));
    let out = f();
    PINNED.with(|p| *p.borrow_mut() = before);
    out
}

/// The token file's text, as it is now; or as this thread's action's guard read it.
pub fn read_text() -> Result<String, TokenError> {
    if let Some(t) = PINNED.with(|p| p.borrow().clone()) {
        return Ok(t);
    }
    let home = std::env::var_os("HOME").ok_or(TokenError::NoHome)?;
    let path = std::path::Path::new(&home).join(".config/factory/bitbucket-token");
    std::fs::read_to_string(&path).map_err(|source| TokenError::Read { path: path.clone(), source })
}

/// Reads `~/.config/factory/merge-token`, Andrei's own in F-B0's form (fac-98): read by the call
/// it serves alone, and never asked of Bitbucket whose it is.
pub fn read_merge() -> Result<Token, TokenError> {
    let home = std::env::var_os("HOME").ok_or(TokenError::NoHome)?;
    let path = std::path::Path::new(&home).join(".config/factory/merge-token");
    parse(&std::fs::read_to_string(&path).map_err(|source| TokenError::Read { path: path.clone(), source })?)
}

pub(crate) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
