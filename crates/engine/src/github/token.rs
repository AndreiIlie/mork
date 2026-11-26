use crate::forge::Result;
use mork_types::work::ActionFailure;

std::thread_local! {
    static PINNED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn read_file(path: &str) -> Result<String> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| ActionFailure::Token { words: format!("GitHub token file: {e}") })?;
    let token = contents.trim();
    if token.is_empty() || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ActionFailure::Token { words: "The GitHub token file must contain one token".into() });
    }
    Ok(token.to_owned())
}

pub(super) fn read(path: &str) -> Result<String> {
    PINNED.with(|p| p.borrow().clone()).map_or_else(|| read_file(path), Ok)
}

pub(crate) fn pinned<T>(token: &str, f: impl FnOnce() -> T) -> T {
    let previous = PINNED.with(|p| p.replace(Some(token.to_owned())));
    let result = f();
    PINNED.with(|p| p.replace(previous));
    result
}

pub(crate) fn git_header(token: &str) -> String {
    format!("Basic {}", crate::encoding::base64(format!("x-access-token:{token}").as_bytes()))
}

pub(crate) fn source(
    owner: &str,
    repo: &str,
    path: &str,
) -> std::result::Result<(String, Vec<(String, String)>), String> {
    let token = read(path).map_err(|e| format!("{e:?}"))?;
    Ok((format!("https://github.com/{owner}/{repo}.git"), crate::git::token_env(&git_header(&token))))
}
