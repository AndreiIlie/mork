//! Local HTTP and WebSocket bridge (fac-671): serves static UI assets and relays `/ws` to `mork.sock`.

pub mod assets;
pub mod frames;
pub mod http;
pub mod images;
pub mod key;
pub mod relay;

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths::Home;

/// Starts the localhost bridge on `127.0.0.1:port` in its own accept thread; logs and returns `None` on failure.
pub fn start(home: &Home, port: u16, assets: Option<PathBuf>) -> Option<SocketAddr> {
    let secret = match key::load_or_create(&home.web_key()) {
        Ok(k) => Arc::<str>::from(k),
        Err(e) => {
            report_error(home, &format!("web bridge key failed: {e}"));
            return None;
        }
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            report_error(home, &format!("web bridge bind 127.0.0.1:{port} failed: {e}"));
            return None;
        }
    };
    let addr = match listener.local_addr() {
        Ok(a) => a,
        Err(e) => {
            report_error(home, &format!("web bridge address failed: {e}"));
            return None;
        }
    };
    crate::runtime::upkeep::log(home, &format!("web bridge listening on http://{addr}"));
    let assets = Arc::new(assets);
    let home = home.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let (secret, assets, home) = (Arc::clone(&secret), Arc::clone(&assets), home.clone());
            std::thread::spawn(move || handle(stream, &secret, assets.as_deref(), &home));
        }
    });
    Some(addr)
}

fn report_error(home: &Home, msg: &str) {
    eprintln!("mork: {msg}");
    crate::runtime::upkeep::log(home, msg);
}

/// Pure dispatch decision for one parsed HTTP request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Ws(http::WsCheck),
    Upload,
    Image { head_only: bool },
    Static { head_only: bool },
    Status(u16, &'static str),
}

/// Routes `/ws` exclusively to the WebSocket handler, `/images/` to a message's image, which takes the key
/// (fac-696), and every other path to static pages.
pub fn route(head: &http::Head, secret: &str) -> Route {
    if head.path == "/ws" {
        return Route::Ws(http::check_ws_upgrade(head, secret));
    }
    if head.path.starts_with("/images/") {
        return match head.method.as_str() {
            _ if !http::bearer_matches(head, secret) => Route::Status(401, "Unauthorized"),
            _ if !http::origin_allowed(head.header("origin")) => Route::Status(403, "Forbidden"),
            "POST" if head.path == images::PASTE => Route::Upload,
            "GET" => Route::Image { head_only: false },
            "HEAD" => Route::Image { head_only: true },
            _ => Route::Status(405, "Method Not Allowed"),
        };
    }
    match head.method.as_str() {
        "GET" => Route::Static { head_only: false },
        "HEAD" => Route::Static { head_only: true },
        _ => Route::Status(405, "Method Not Allowed"),
    }
}

fn handle(mut stream: TcpStream, secret: &str, assets: Option<&Path>, home: &Home) {
    let Ok(Some((head, leftover))) = http::read_head(&mut stream) else { return };
    match route(&head, secret) {
        Route::Ws(http::WsCheck::Status(code, reason)) | Route::Status(code, reason) => {
            let _ = http::write_status(&mut stream, code, reason);
        }
        Route::Ws(http::WsCheck::Ok { ws_key }) => {
            let accept = frames::accept_key(&ws_key);
            if http::write_101(&mut stream, &accept).is_ok() {
                relay::serve(stream, leftover, home.mork_sock());
            }
        }
        Route::Upload => {
            let _ = images::upload(&mut stream, &head, leftover, home);
        }
        Route::Image { head_only } => match images::open_image(home, &head.path) {
            Some((file, content_type)) => {
                let _ = http::write_file(&mut stream, content_type, file, head_only);
            }
            None => {
                let _ = http::write_status(&mut stream, 404, "Not Found");
            }
        },
        Route::Static { head_only } => handle_static(&mut stream, &head.path, assets, head_only),
    }
}

fn handle_static(stream: &mut TcpStream, url_path: &str, assets: Option<&Path>, head_only: bool) {
    let Some(root) = assets else {
        let _ = http::write_status(stream, 404, "Not Found");
        return;
    };
    match assets::open_asset(root, url_path) {
        Some((file, content_type)) => {
            let _ = http::write_file(stream, content_type, file, head_only);
        }
        None => {
            let _ = http::write_status(stream, 404, "Not Found");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn head(raw: &str) -> http::Head {
        http::parse_head(raw.as_bytes()).unwrap()
    }

    #[test]
    fn an_image_takes_the_key() {
        let bare = head("GET /images/j/1-a.png HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(route(&bare, KEY), Route::Status(401, "Unauthorized"));
        let in_url = head(&format!("GET /images/j/1-a.png?key={KEY} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"));
        assert_eq!(route(&in_url, KEY), Route::Status(401, "Unauthorized"));
        let keyed = head(&format!("GET /images/j/1-a.png HTTP/1.1\r\nAuthorization: Bearer {KEY}\r\n\r\n"));
        assert_eq!(route(&keyed, KEY), Route::Image { head_only: false });
    }

    #[test]
    fn paste_upload_checks_key_origin_and_method() {
        let post = |headers: &str| head(&format!("POST /images/pasted HTTP/1.1\r\n{headers}\r\n"));
        let auth = format!("Authorization: Bearer {KEY}\r\n");
        assert_eq!(route(&post(""), KEY), Route::Status(401, "Unauthorized"));
        assert_eq!(route(&post(&format!("{auth}Origin: null\r\n")), KEY), Route::Status(403, "Forbidden"));
        assert_eq!(route(&post(&auth), KEY), Route::Upload);
        let other = head(&format!("POST /images/j/file HTTP/1.1\r\n{auth}\r\n"));
        assert_eq!(route(&other, KEY), Route::Status(405, "Method Not Allowed"));
    }

    #[test]
    fn route_keeps_ws_exclusive_and_dispatches_static_methods() {
        let plain_ws = head(&format!("GET /ws?key={KEY} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"));
        assert_eq!(route(&plain_ws, KEY), Route::Ws(http::WsCheck::Status(426, "Upgrade Required")));

        let unauth_ws = head("GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(route(&unauth_ws, KEY), Route::Ws(http::WsCheck::Status(401, "Unauthorized")));

        let get_page = head("GET /cards/fac-671 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(route(&get_page, KEY), Route::Static { head_only: false });

        let head_page = head("HEAD / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(route(&head_page, KEY), Route::Static { head_only: true });

        let post_page = head("POST / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(route(&post_page, KEY), Route::Status(405, "Method Not Allowed"));
    }
}
