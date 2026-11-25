//! The two sockets (1.6): `factory.sock`, Andrei's, and `run/agent.sock`, the agents'. Until the
//! engine is ready, every request is answered `recovering` here, without reaching it.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use factory_types::protocol::{Answer, ErrorBody, ErrorCode, Hello, HelloLine, MAX_LINE, Request};
use factory_types::state::Sender;

/// Which socket a connection came in on: the sender's identity (1.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Andrei,
    Agent,
}

// The lines a connection may have waiting to be written; a client that lets more pile up
// isn't reading, and is dropped (1.1.3).
const QUEUE: usize = 256;

#[derive(Debug)]
struct Outgoing {
    line: String,
    written: Option<mpsc::Sender<()>>,
}

/// One open connection. Its lines are written by a thread of its own, so the engine never
/// waits on a client.
#[derive(Debug)]
pub struct Conn {
    pub id: u64,
    pub origin: Origin,
    pub peer_pid: i32,
    queue: Mutex<Option<SyncSender<Outgoing>>>,
    stream: UnixStream,
    open: AtomicBool,
    // Lines queued and lines written, counted from its `hello`: an orchestrator's hook reads what it was
    // written (fac-119).
    queued: AtomicU64,
    written: AtomicU64,
}

impl Conn {
    fn new(id: u64, origin: Origin, peer_pid: i32, stream: UnixStream) -> io::Result<Arc<Self>> {
        let (tx, rx) = mpsc::sync_channel::<Outgoing>(QUEUE);
        let writer = stream.try_clone()?;
        let conn = Arc::new(Self {
            id,
            origin,
            peer_pid,
            queue: Mutex::new(Some(tx)),
            stream,
            open: AtomicBool::new(true),
            queued: AtomicU64::new(0),
            written: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&conn);
        std::thread::spawn(move || {
            let mut w = writer;
            for Outgoing { line, written } in rx {
                let wrote = w.write_all(line.as_bytes()).and_then(|()| w.write_all(b"\n")).and_then(|()| w.flush());
                let conn = weak.upgrade();
                if wrote.is_err() {
                    if let Some(c) = conn {
                        c.close();
                    }
                    break;
                }
                if let Some(c) = conn {
                    c.written.fetch_add(1, Ordering::SeqCst);
                }
                if let Some(written) = written {
                    let _ = written.send(());
                }
            }
            // Its queue ended (`finish`): what was queued is written, and the client reads its end.
            let _ = w.shutdown(std::net::Shutdown::Write);
        });
        Ok(conn)
    }

    /// Queues one line, and says whether it was: a client whose queue is full is dropped.
    pub fn send_line(&self, line: &str) -> bool {
        self.enqueue(Outgoing { line: line.to_owned(), written: None })
    }

    fn enqueue(&self, outgoing: Outgoing) -> bool {
        let Ok(mut q) = self.queue.lock() else { return false };
        let full = match q.as_ref().map(|tx| tx.try_send(outgoing)) {
            Some(Ok(())) => false,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_))) | None => true,
        };
        if full {
            *q = None;
            drop(q);
            self.close();
            return false;
        }
        self.queued.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// How many lines were queued, and how many of them written.
    pub fn counts(&self) -> (u64, u64) {
        (self.queued.load(Ordering::SeqCst), self.written.load(Ordering::SeqCst))
    }

    /// Ends its queue: the lines already queued are written, then its writing side is closed.
    pub fn finish(&self) {
        if let Ok(mut q) = self.queue.lock() {
            *q = None;
        }
    }

    // Its reader ends, and its leases with it.
    fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn answer(&self, id: u64, result: Result<serde_json::Value, ErrorBody>) {
        if let Ok(line) = answer_line(id, result) {
            self.enqueue(Outgoing { line, written: None });
        }
    }

    /// Sends the final answer. The receiver gets `()` after the complete write or disconnects on failure.
    pub fn answer_and_finish(&self, id: u64, ok: serde_json::Value) -> mpsc::Receiver<()> {
        let (written, outcome) = mpsc::channel();
        if let Ok(line) = answer_line(id, Ok(ok)) {
            self.enqueue(Outgoing { line, written: Some(written) });
        }
        self.finish();
        outcome
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }
}

fn answer_line(id: u64, result: Result<serde_json::Value, ErrorBody>) -> serde_json::Result<String> {
    serde_json::to_string(&match result {
        Ok(ok) => Answer::Ok { id, ok },
        Err(err) => Answer::Err { id, err },
    })
}

/// What the readers send the engine.
#[derive(Debug)]
pub enum Incoming {
    /// A request, with its sender: `None` for a peer of `agent.sock` in no running job; and an
    /// agent's job, whose wall it came from.
    Request {
        conn: Arc<Conn>,
        req: Request,
        sender: Option<Sender>,
        job: Option<factory_types::JobId>,
    },
    Closed {
        conn: u64,
    },
}

/// What every connection's `hello` and the readers share with the engine.
#[derive(Debug)]
pub struct Shared {
    pub build: String,
    pub dirty: bool,
    pub home: String,
    /// The open state's version, or none while it is being opened (1.6.3).
    pub state_version: Mutex<Option<u32>>,
    /// Commands reach the engine only once recovery has ended (2.7.2).
    pub ready: AtomicBool,
    /// The home's `jobs/`, where a peer of `agent.sock` is looked up (1.5.4).
    pub jobs_dir: std::path::PathBuf,
    next_conn: AtomicU64,
}

impl Shared {
    pub fn new(build: String, dirty: bool, home: String) -> Arc<Self> {
        let jobs_dir = std::path::PathBuf::from(&home).join("jobs");
        Arc::new(Self {
            build,
            dirty,
            home,
            jobs_dir,
            state_version: Mutex::new(None),
            ready: AtomicBool::new(false),
            next_conn: AtomicU64::new(1),
        })
    }

    fn hello(&self) -> String {
        let state_version = self.state_version.lock().ok().and_then(|v| *v);
        let h = HelloLine {
            hello: Hello {
                build: self.build.clone(),
                dirty: self.dirty,
                protocol: factory_types::protocol::PROTOCOL,
                state_version,
                home: self.home.clone(),
            },
        };
        serde_json::to_string(&h).unwrap_or_default()
    }
}

/// Binds a socket at `path`, mode 0600, replacing a stale one (the app lock is held).
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let l = UnixListener::bind(path)?;
    factory_sys::chmod(path, 0o600)?;
    Ok(l)
}

/// Accepts connections on a thread of its own.
pub fn serve(listener: UnixListener, origin: Origin, shared: Arc<Shared>, tx: mpsc::Sender<Incoming>) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = Arc::clone(&shared);
            let tx = tx.clone();
            std::thread::spawn(move || connection(stream, origin, &shared, &tx));
        }
    });
}

fn connection(stream: UnixStream, origin: Origin, shared: &Shared, tx: &mpsc::Sender<Incoming>) {
    let peer_pid = factory_sys::peer_pid(&stream).unwrap_or(0);
    let Ok(reader) = stream.try_clone() else { return };
    let Ok(conn) = Conn::new(shared.next_conn.fetch_add(1, Ordering::SeqCst), origin, peer_pid, stream) else { return };
    conn.send_line(&shared.hello());
    let mut reader = BufReader::new(reader);
    let mut sender: Option<Option<(Sender, Option<factory_types::JobId>)>> = None;
    loop {
        match read_line(&mut reader) {
            Ok(None) => break,
            Ok(Some(Line::TooLarge)) => {
                conn.answer(0, Err(ErrorBody::new(ErrorCode::TooLarge, "the line is over 1 MiB")))
            }
            Ok(Some(Line::NotUtf8)) => conn.answer(0, Err(ErrorBody::new(ErrorCode::Invalid, "the line isn't UTF-8"))),
            Ok(Some(Line::Text(text))) => handle_line(&conn, shared, tx, &text, &mut sender),
            Err(_) => break,
        }
    }
    conn.open.store(false, Ordering::SeqCst);
    let _ = tx.send(Incoming::Closed { conn: conn.id });
}

fn handle_line(
    conn: &Arc<Conn>,
    shared: &Shared,
    tx: &mpsc::Sender<Incoming>,
    text: &str,
    sender: &mut Option<Option<(Sender, Option<factory_types::JobId>)>>,
) {
    let req: Request = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let id =
                serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v.get("id")?.as_u64()).unwrap_or(0);
            conn.answer(id, Err(ErrorBody::new(ErrorCode::Invalid, format!("request: {e}"))));
            return;
        }
    };
    if !shared.ready.load(Ordering::SeqCst) {
        conn.answer(req.id, Err(ErrorBody::new(ErrorCode::Recovering, "the factory is recovering")));
        return;
    }
    // The sender, known once per connection: the socket says Andrei unless the peer runs in an
    // unwalled job, or the wall says which agent (looked up here, never on the engine's thread).
    let who = sender
        .get_or_insert_with(|| match conn.origin {
            Origin::Andrei => Some(
                crate::jobs::identity::of_unwalled_peer(&shared.jobs_dir, conn.peer_pid)
                    .map_or((Sender::Andrei, None), |(agent, job)| (Sender::Agent(agent), Some(job))),
            ),
            Origin::Agent => crate::jobs::identity::of_peer(&shared.jobs_dir, conn.peer_pid)
                .map(|(agent, job)| (Sender::Agent(agent), Some(job))),
        })
        .clone();
    let (sender, job) = who.map_or((None, None), |(s, j)| (Some(s), j));
    let _ = tx.send(Incoming::Request { conn: Arc::clone(conn), req, sender, job });
}

enum Line {
    Text(String),
    TooLarge,
    NotUtf8,
}

fn read_line<R: BufRead>(r: &mut R) -> io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let n = io::Read::take(&mut *r, MAX_LINE as u64 + 1).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > MAX_LINE {
        // The rest of the line is skipped through the reader's own buffer, never kept.
        loop {
            let chunk = r.fill_buf()?;
            if chunk.is_empty() {
                return Ok(Some(Line::TooLarge));
            }
            match chunk.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    r.consume(i + 1);
                    return Ok(Some(Line::TooLarge));
                }
                None => {
                    let n = chunk.len();
                    r.consume(n);
                }
            }
        }
    }
    match String::from_utf8(buf) {
        Ok(text) => Ok(Some(Line::Text(text.trim_end_matches(['\n', '\r']).to_owned()))),
        Err(_) => Ok(Some(Line::NotUtf8)),
    }
}
