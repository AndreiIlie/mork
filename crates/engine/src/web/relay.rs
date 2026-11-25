//! Full-duplex relay between one upgraded WebSocket and one `factory.sock` connection (fac-671).

use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use factory_types::protocol::MAX_LINE;

use super::frames::{self, Message, MessageReader, ReadError};

struct Writer {
    stream: TcpStream,
    closed: bool,
}

/// Relays one JSON line per WebSocket text frame both ways until either side closes or errors.
pub fn serve(stream: TcpStream, leftover: Vec<u8>, sock_path: PathBuf) {
    let _ = stream.set_read_timeout(None);
    let (Ok(writer_clone), Ok(tcp_ctrl)) = (stream.try_clone(), stream.try_clone()) else { return };
    let writer = Arc::new(Mutex::new(Writer { stream: writer_clone, closed: false }));
    let tcp_ctrl = Arc::new(tcp_ctrl);
    let peer_closing = Arc::new(AtomicBool::new(false));
    let Ok(unix) = UnixStream::connect(&sock_path) else {
        send_close(&writer, 1011);
        let _ = tcp_ctrl.shutdown(Shutdown::Both);
        return;
    };
    let Ok(unix_reader) = unix.try_clone() else {
        let _ = tcp_ctrl.shutdown(Shutdown::Both);
        return;
    };
    let (to_ws, ctrl, closing) = (Arc::clone(&writer), Arc::clone(&tcp_ctrl), Arc::clone(&peer_closing));
    let forward = std::thread::spawn(move || sock_to_ws(unix_reader, &to_ws, &ctrl, &closing));
    ws_to_sock(stream, leftover, unix, &writer, &tcp_ctrl, &peer_closing);
    let _ = forward.join();
}

fn sock_to_ws(unix: UnixStream, writer: &Mutex<Writer>, tcp_ctrl: &TcpStream, peer_closing: &AtomicBool) {
    let mut reader = BufReader::new(unix);
    let mut line = String::new();
    let mut tcp_failed = false;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = line.trim_end_matches(['\n', '\r']);
                if write_open(writer, |w| frames::write_text(w, text)).is_err() {
                    tcp_failed = true;
                    break;
                }
            }
        }
    }
    // When ws_to_sock initiated a Close handshake, let it send its Close code before TCP shutdown.
    if !peer_closing.load(Ordering::SeqCst) {
        if !tcp_failed {
            send_close(writer, 1000);
        }
        let _ = tcp_ctrl.shutdown(Shutdown::Both);
    }
    let _ = reader.into_inner().shutdown(Shutdown::Both);
}

fn ws_to_sock(
    stream: TcpStream,
    leftover: Vec<u8>,
    mut unix: UnixStream,
    writer: &Mutex<Writer>,
    tcp_ctrl: &TcpStream,
    peer_closing: &AtomicBool,
) {
    let mut reader = BufReader::new(Cursor::new(leftover).chain(stream));
    let mut frames_in = MessageReader::new(MAX_LINE);
    let close_code = loop {
        match frames_in.read(&mut reader) {
            Ok(Some(Message::Text(text))) => {
                if unix
                    .write_all(text.as_bytes())
                    .and_then(|()| unix.write_all(b"\n"))
                    .and_then(|()| unix.flush())
                    .is_err()
                {
                    break None;
                }
            }
            Ok(Some(Message::Ping(data))) => {
                if write_open(writer, |w| frames::write_pong(w, &data)).is_err() {
                    break None;
                }
            }
            Ok(Some(Message::Pong(_))) => {}
            Ok(Some(Message::Close)) => break Some(1000),
            Ok(None) | Err(ReadError::Io(_)) => break None,
            Err(ReadError::Protocol(code)) => break Some(code),
        }
    };
    if let Some(code) = close_code {
        peer_closing.store(true, Ordering::SeqCst);
        let _ = unix.shutdown(Shutdown::Both);
        send_close(writer, code);
    } else {
        let _ = unix.shutdown(Shutdown::Both);
    }
    let _ = tcp_ctrl.shutdown(Shutdown::Both);
}

fn write_open(writer: &Mutex<Writer>, f: impl FnOnce(&mut TcpStream) -> io::Result<()>) -> io::Result<()> {
    let mut guard = writer.lock().map_err(|_| io::Error::other("writer lock poisoned"))?;
    if guard.closed {
        return Err(io::Error::from(io::ErrorKind::BrokenPipe));
    }
    let res = f(&mut guard.stream);
    if res.is_err() {
        guard.closed = true;
    }
    res
}

fn send_close(writer: &Mutex<Writer>, code: u16) {
    if let Ok(mut guard) = writer.lock()
        && !guard.closed
    {
        guard.closed = true;
        let _ = frames::write_close(&mut guard.stream, code);
    }
}
