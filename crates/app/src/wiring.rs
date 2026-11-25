//! Kept apart from iced's runtime so the tests drive the window through it as the app does (6.3).
//! A command is sent when it is typed, recovering or not (5.1.3, M4.8).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use factory_engine::paths::Home;
use factory_types::Command;
use factory_types::view::{Phase, PlaceView, Screen};
use factory_ui::app::{App, Effect, Message as UiMessage, WindowOp};
use factory_ui::session::UiSession;
use iced::futures::channel::mpsc as fmpsc;
use serde_json::{Value, json};

/// What reaches the window: its own messages, and what the engine sent.
#[derive(Debug, Clone)]
pub enum Msg {
    Ui(UiMessage),
    Screen(Box<Screen>),
    Place(Box<PlaceView>),
    Answer(Result<Value, String>),
    Tick(i64),
    Recovering(String),
    /// The window was asked to close, however (the ×, Alt+F4, the dock).
    CloseRequested,
}

/// What the window sends the engine.
#[derive(Debug, Clone)]
pub enum Out {
    Command(Box<Command>),
    Watch(String),
}

/// A message to the window: what it asks done in return.
pub fn step(app: &mut App, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Ui(m) => return app.update(m),
        Msg::Screen(screen) => app.set_screen(*screen),
        Msg::Place(v) => app.set_place(*v),
        Msg::Answer(a) => app.answered(a),
        Msg::Tick(now) => app.tick(now),
        Msg::CloseRequested => return vec![Effect::Window(WindowOp::Close)],
        Msg::Recovering(doing) => {
            let mut screen = app.screen.clone();
            screen.top.phase = Phase::Recovering { doing };
            app.set_screen(screen);
        }
    }
    Vec::new()
}

/// A line from the engine, as the window's message: a screen, a place's view, or the answer to
/// a command it sent (ids from 2; 0 and 1 are the link's own).
pub fn msg_of(line: &str) -> Option<Msg> {
    let v = serde_json::from_str::<Value>(line).ok()?;
    if let Some(s) = v.pointer("/event/screen") {
        serde_json::from_value(s.clone()).ok().map(|s| Msg::Screen(Box::new(s)))
    } else if let Some(p) = v.pointer("/event/view") {
        serde_json::from_value(p.clone()).ok().map(|p| Msg::Place(Box::new(p)))
    } else if v["id"].as_u64().is_some_and(|id| id > 1) {
        Some(Msg::Answer(match (v.get("ok"), v.get("err")) {
            (Some(ok), _) => Ok(ok.clone()),
            (None, e) => Err(e.and_then(|e| e["message"].as_str()).unwrap_or("refused").to_owned()),
        }))
    } else {
        None
    }
}

/// `ui-session.json`, when this build can read it; else a fresh window on main (2.3.3).
pub fn read_session(home: &Home) -> UiSession {
    std::fs::read(home.ui_session())
        .ok()
        .and_then(|b| serde_json::from_slice::<UiSession>(&b).ok())
        .filter(|s| s.format == factory_ui::session::FORMAT)
        .unwrap_or_default()
}

/// Writes `ui-session.json`: the window keeps it current, so a reload or a restart finds it
/// (5.1.5, M4.7).
pub fn save_session(home: &Home, session: &UiSession) {
    // Written whole, under a temporary name first: another build reads it (1.3.1, M4.25).
    if let Ok(text) = serde_json::to_vec(session) {
        let _ = factory_engine::files::write_whole(&home.ui_session(), &text);
    }
}

/// Starts the config's editor on a finding only if `path`, read through its symlinks, stays inside `clone`
/// (5.7.5); a finding is accepted as its agent wrote it. Returns whether it started.
pub fn editor(home: &Home, argv: &[String], clone: &str, path: &str) -> bool {
    let inside = std::fs::canonicalize(clone)
        .and_then(|root| std::fs::canonicalize(path).map(|real| real.starts_with(&root) && real.is_file()));
    if !matches!(inside, Ok(true)) {
        eprintln!("factory: {path} isn't a file of its clone: no editor started");
        return false;
    }
    if let Err(e) = factory_engine::jobs::launch::editor(home.root(), argv) {
        eprintln!("factory: the editor didn't start: {e}");
        return false;
    }
    true
}

/// An HTTP or HTTPS page, in the system's browser.
pub fn page(home: &Home, url: &str) -> bool {
    if !web_url(url) {
        eprintln!("factory: {url:?} isn't a web page's address: nothing opened");
        return false;
    }
    if let Err(e) = factory_engine::jobs::launch::browser(home.root(), url) {
        eprintln!("factory: the browser didn't start: {e}");
        return false;
    }
    true
}

fn web_url(url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Reports the window's owned forwards over its inbox, without exposing a socket command.
pub fn tunnels(
    home: Home,
    configured: std::collections::BTreeMap<String, factory_types::config::TunnelConfig>,
    tx: fmpsc::UnboundedSender<Msg>,
) -> mpsc::Sender<factory_engine::jobs::web_tunnels::Request> {
    factory_engine::jobs::web_tunnels::start(home, configured, move |name, status| {
        tx.unbounded_send(Msg::Ui(UiMessage::TunnelStatus(name, status))).is_ok()
    })
}

/// A file a card's agent turn produced, in the system's viewer (fac-105): a copy of a regular
/// file beneath its seat's `io/` folder `seat` (fac-654), of a kind the viewer shows. Why it wasn't, for the window.
pub fn output(home: &Home, seat: &str, name: &str) -> Result<(), String> {
    factory_engine::jobs::outputs::open(home, seat, name).map_err(|e| {
        let why = format!("{name} of the io/ folder {seat} wasn't opened: {e}");
        eprintln!("factory: {why}");
        why
    })
}

/// A message's image in the system's viewer (fac-278): a copy of a PNG or JPEG a reply published or
/// a seat named in its `io/` folder, or one Andrei pasted. Why it wasn't, for the window.
pub fn image(home: &Home, path: &str) -> Result<(), String> {
    factory_engine::jobs::outputs::open_image(home, path).map_err(|e| {
        let why = format!("{path} wasn't opened: {e}");
        eprintln!("factory: {why}");
        why
    })
}

/// The window's connection to `factory.sock`, on a thread: made again whenever it closes (a
/// reload, a restart).
pub fn link(home: Home, tx: fmpsc::UnboundedSender<Msg>, outs: mpsc::Receiver<Out>, mut place: String) {
    std::thread::spawn(move || {
        let outs = outs;
        loop {
            if let Ok(stream) = UnixStream::connect(home.factory_sock()) {
                serve(stream, &tx, &outs, &mut place);
            }
            factory_engine::clock::sleep(Duration::from_millis(300));
        }
    });
}

// The link's `status` (id 0) is answered `recovering` until the engine is up.
fn reader(
    stream: &UnixStream,
    tx: &fmpsc::UnboundedSender<Msg>,
    ready: &Arc<AtomicBool>,
) -> Option<std::thread::JoinHandle<()>> {
    let read = stream.try_clone().ok()?;
    let (tx, ready) = (tx.clone(), Arc::clone(ready));
    Some(std::thread::spawn(move || {
        for line in BufReader::new(read).lines().map_while(Result::ok) {
            let v: Value = serde_json::from_str(&line).unwrap_or_default();
            let msg = if v["id"] == 0 {
                if v["err"]["code"] == "recovering" {
                    Some(Msg::Recovering(v["err"]["message"].as_str().unwrap_or("").to_owned()))
                } else {
                    ready.store(true, Ordering::SeqCst);
                    None
                }
            } else {
                msg_of(&line)
            };
            if let Some(m) = msg
                && tx.unbounded_send(m).is_err()
            {
                return;
            }
        }
    }))
}

// Each command goes out when the window asks, before `ready` too: the engine answers `recovering` (5.1.3).
fn serve(stream: UnixStream, tx: &fmpsc::UnboundedSender<Msg>, outs: &mpsc::Receiver<Out>, place: &mut String) {
    let ready = Arc::new(AtomicBool::new(false));
    let Some(reader) = reader(&stream, tx, &ready) else { return };
    let mut stream = stream;
    let send = |stream: &mut UnixStream, id: u64, cmd: Value| {
        stream.write_all(format!("{}\n", json!({"id": id, "cmd": cmd})).as_bytes()).is_ok()
    };
    // A place's view carries its latest messages; the window asks for the pages before (fac-42).
    let tail = factory_ui::app::PAGE;
    let watch = |place: &str| json!({"watch": {"places": [factory_engine::runtime::SCREEN, place], "tail": tail, "full": true}});
    let (mut next, mut watching, mut probed) = (2u64, false, None::<std::time::Instant>);
    while !reader.is_finished() {
        let mut ok = true;
        if !watching && ready.load(Ordering::SeqCst) {
            watching = true;
            ok = send(&mut stream, 1, watch(place));
        } else if !watching && probed.is_none_or(|t| factory_engine::clock::elapsed(t) >= Duration::from_millis(500)) {
            probed = Some(factory_engine::clock::instant());
            ok = send(&mut stream, 0, json!({"status": {}}));
        }
        ok &= match outs.recv_timeout(Duration::from_millis(100)) {
            Ok(Out::Watch(p)) => {
                *place = p;
                !watching || send(&mut stream, 1, watch(place))
            }
            Ok(Out::Command(cmd)) => {
                next += 1;
                send(&mut stream, next, serde_json::to_value(&cmd).unwrap_or_default())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if !ok {
            return;
        }
    }
}
