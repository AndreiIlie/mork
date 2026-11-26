// The engine runs on a thread of its own; the window is a client of `mork.sock` like any other (1.6).

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use iced::futures::channel::mpsc as fmpsc;
use iced::widget::text_editor::{Action, Edit};
use iced::{Element, Subscription, Task};
use mork_app::wiring::{self, Msg, Out};
use mork_engine::paths::Home;
use mork_types::view::Screen;
use mork_ui::app::{App, Effect, Message, Settings, WindowOp};

static INBOX: OnceLock<Mutex<Option<fmpsc::UnboundedReceiver<Msg>>>> = OnceLock::new();

struct Shell {
    app: App,
    out: mpsc::Sender<Out>,
    home: Home,
    tunnels: mpsc::Sender<mork_engine::jobs::web_tunnels::Request>,
}

/// Runs the engine and the window; returns only if the window couldn't run.
pub fn run(
    home: &Home,
    settings: Settings,
    engine: impl FnOnce(mpsc::Sender<mork_engine::jobs::web_tunnels::Configured>) -> i32 + Send + 'static,
) -> i32 {
    let (ports, configured) = mpsc::channel();
    std::thread::spawn(move || std::process::exit(engine(ports)));
    let (tx, rx) = fmpsc::unbounded();
    let _ = INBOX.set(Mutex::new(Some(rx)));
    let (out, outs) = mpsc::channel();
    let session = wiring::read_session(home);
    let place = session.place.clone();
    let window = mork_ui::window::settings_at(session.position);
    wiring::link(home.clone(), tx.clone(), outs, place);
    let tunnels = wiring::tunnels(home.clone(), configured, tx.clone());
    clock(tx);
    let home = home.clone();
    let boot = move || {
        let app = App::new(Screen::default(), session.clone(), settings.clone());
        (Shell { app, out: out.clone(), home: home.clone(), tunnels: tunnels.clone() }, mork_ui::window::focus_input())
    };
    let result = iced::application(boot, update, view)
        // Avoid the reported NVIDIA/Wayland frame stall (fac-120, iced-rs/iced#3496).
        .settings(iced::Settings { vsync: false, ..Default::default() })
        .subscription(subscription)
        .window(window)
        .font(mork_ui::fonts::REGULAR_BYTES)
        .font(mork_ui::fonts::BOLD_BYTES)
        .font(mork_ui::fonts::ITALIC_BYTES)
        .font(mork_ui::fonts::ICONS_BYTES)
        .default_font(mork_ui::fonts::MONO)
        .theme(|_: &Shell| mork_ui::theme::iced_theme())
        .title(|_: &Shell| "mork".to_owned())
        .run();
    if let Err(e) = result {
        eprintln!("mork: the window couldn't run: {e}");
    }
    // However the window ended, the engine ends cleanly and exits the process (2.7).
    end_app();
    loop {
        std::thread::park();
    }
}

fn view(s: &Shell) -> Element<'_, Msg> {
    s.app.view().map(Msg::Ui)
}

fn subscription(_: &Shell) -> Subscription<Msg> {
    Subscription::batch([
        Subscription::run(inbox),
        iced::window::close_requests().map(|_| Msg::CloseRequested),
        // Where the window is: it opens there next time, on the same monitor (5.1.1, M4.16).
        iced::window::events().filter_map(|(_, e)| match e {
            // why: a desktop's coordinates are whole pixels, far inside i32.
            #[allow(clippy::cast_possible_truncation)]
            iced::window::Event::Moved(p) => {
                Some(Msg::Ui(mork_ui::app::Message::Moved(p.x.round() as i32, p.y.round() as i32)))
            }
            _ => None,
        }),
    ])
}

// As SIGTERM does: the engine shuts down cleanly first (2.7).
fn end_app() {
    let _ = mork_sys::kill(mork_sys::getpid(), 15);
}

fn inbox() -> impl iced::futures::Stream<Item = Msg> {
    let rx = INBOX.get().and_then(|m| m.lock().ok().and_then(|mut r| r.take()));
    let (_, empty) = fmpsc::unbounded();
    rx.unwrap_or(empty)
}

fn update(s: &mut Shell, msg: Msg) -> Task<Msg> {
    // The clock stands still while nothing moves: anything else that happens reads it fresh (a
    // double click's timing, when a hand was first seen).
    if !matches!(msg, Msg::Tick(_)) {
        s.app.tick(mork_engine::clock::now_ms());
    }
    let effects = wiring::step(&mut s.app, msg);
    FRAME_EVERY.store(s.app.frame_every().unwrap_or(0), Ordering::Relaxed);
    Task::batch(effects.into_iter().map(|e| effect(s, e)))
}

fn effect(s: &Shell, e: Effect) -> Task<Msg> {
    match e {
        Effect::Send(cmd) => {
            let _ = s.out.send(Out::Command(Box::new(cmd)));
            Task::none()
        }
        Effect::Watch(place) => {
            let _ = s.out.send(Out::Watch(place));
            Task::none()
        }
        Effect::Editor { argv, clone, path } => {
            wiring::editor(&s.home, &argv, &clone, &path);
            Task::none()
        }
        Effect::Output { job, name } => output(s.home.clone(), job, name),
        Effect::Image(path) => viewed(move |home| wiring::image(home, &path), s.home.clone()),
        Effect::Link(url) => {
            wiring::page(&s.home, &url);
            Task::none()
        }
        Effect::Tunnel(name, open) => {
            if s.tunnels.send((name.clone(), open)).is_err() {
                return Task::done(Msg::Ui(Message::TunnelStatus(
                    name,
                    mork_types::web_tunnel::Status::Failed("The tunnel tracker stopped".into()),
                )));
            }
            Task::none()
        }
        Effect::SaveSession(session) => {
            wiring::save_session(&s.home, &session);
            Task::none()
        }
        Effect::Paste { place } => paste(s.home.clone(), place),
        Effect::Copy(words) => iced::clipboard::write(words),
        // Closing the window ends the app as SIGTERM does: the engine shuts down cleanly first
        // (2.7, 5.1.4), then the process exits.
        Effect::Window(WindowOp::Close) => {
            end_app();
            Task::none()
        }
        Effect::Window(op) => mork_ui::window::window_task(op),
    }
}

// The clipboard's text is pasted as the editor's own paste does; with none, its image is read off the
// window's thread, since the read waits on the clipboard's owner, and joins `place`'s draft (fac-31).
fn paste(home: Home, place: String) -> Task<Msg> {
    iced::clipboard::read().then(move |text| match text.filter(|t| !t.is_empty()) {
        Some(t) => Task::done(Msg::Ui(Message::Edit(Action::Edit(Edit::Paste(Arc::new(t)))))),
        None => {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let home = home.clone();
            std::thread::spawn(move || {
                let _ = tx.send(mork_engine::clipboard::image(&home));
            });
            let place = place.clone();
            Task::future(rx).then(move |read| {
                let image = match read {
                    Ok(Ok(Some(path))) => Ok(path.display().to_string()),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(None)) | Err(_) => return Task::none(),
                };
                Task::done(Msg::Ui(Message::Pasted { place: place.clone(), image }))
            })
        }
    })
}

// A produced file is copied for the viewer off the window's thread, and a refusal comes back to
// the window as its note (fac-105).
fn output(home: Home, job: String, name: String) -> Task<Msg> {
    viewed(move |home| wiring::output(home, &job, &name), home)
}

fn viewed(open: impl FnOnce(&Home) -> Result<(), String> + Send + 'static, home: Home) -> Task<Msg> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(open(&home));
    });
    Task::future(rx).then(|opened| match opened {
        Ok(Err(why)) => Task::done(Msg::Ui(Message::OutputRefused(why))),
        Ok(Ok(())) | Err(_) => Task::none(),
    })
}

// `App::frame_every` as last set, 0 for never: written after each update, read by the clock (M5.36).
static FRAME_EVERY: AtomicI64 = AtomicI64::new(0);

// A tick only as often as something on the window moves by itself, none while nothing does, so an idle
// window isn't drawn again (5.11, M5.36).
fn clock(tx: fmpsc::UnboundedSender<Msg>) {
    std::thread::spawn(move || {
        let mut last = 0;
        loop {
            mork_engine::clock::sleep(Duration::from_millis(25));
            let every = FRAME_EVERY.load(Ordering::Relaxed);
            let now = mork_engine::clock::now_ms();
            if every > 0 && now.saturating_sub(last) >= every {
                last = now;
                if tx.unbounded_send(Msg::Tick(now)).is_err() {
                    return;
                }
            }
        }
    });
}
