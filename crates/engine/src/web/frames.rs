//! Minimal RFC 6455 server-side WebSocket framing and SHA-1 accept key (fac-671).

use std::io::{self, Read, Write};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

const OP_CONT: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// One decoded incoming WebSocket message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// Failure reading a WebSocket message: transport I/O or protocol refusal with close code.
#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    Protocol(u16),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Computes `Sec-WebSocket-Accept` from the client's `Sec-WebSocket-Key` (RFC 6455 §1.3).
pub fn accept_key(client_key: &str) -> String {
    let input = format!("{}{WS_GUID}", client_key.trim());
    let digest = sha1(input.as_bytes());
    crate::encoding::base64(&digest)
}

// Pure-Rust FIPS 180-4 SHA-1 used only for RFC 6455's Sec-WebSocket-Accept handshake value.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in padded.as_chunks::<64>().0 {
        sha1_block(&mut h, block);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha1_block(h: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (i, &chunk) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(chunk);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *h;
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
        (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
    }
    for (slot, delta) in h.iter_mut().zip([a, b, c, d, e]) {
        *slot = slot.wrapping_add(delta);
    }
}

/// Stateful reader assembling fragmented text frames while permitting interleaved control frames.
#[derive(Debug)]
pub struct MessageReader {
    max_len: usize,
    buf: Vec<u8>,
    in_text: bool,
}

impl MessageReader {
    /// Creates a reader capping assembled text messages at `max_len` bytes.
    pub fn new(max_len: usize) -> Self {
        Self { max_len, buf: Vec::new(), in_text: false }
    }

    /// Reads the next complete message; `Ok(None)` on clean EOF at a frame boundary.
    pub fn read<R: Read>(&mut self, r: &mut R) -> Result<Option<Message>, ReadError> {
        loop {
            let Some(frame) = read_frame_header(r)? else { return Ok(None) };
            self.validate_frame(&frame)?;
            let payload = read_masked_payload(r, frame.len, frame.mask)?;
            if let Some(msg) = self.apply_frame(frame.fin, frame.opcode, payload)? {
                return Ok(Some(msg));
            }
        }
    }

    fn validate_frame(&self, frame: &FrameHeader) -> Result<(), ReadError> {
        if frame.rsv != 0 || !frame.masked {
            return Err(ReadError::Protocol(1002));
        }
        if frame.opcode >= 0x8 {
            return if frame.fin && frame.len <= 125 { Ok(()) } else { Err(ReadError::Protocol(1002)) };
        }
        match frame.opcode {
            OP_BINARY => return Err(ReadError::Protocol(1003)),
            OP_TEXT if self.in_text => return Err(ReadError::Protocol(1002)),
            OP_CONT if !self.in_text => return Err(ReadError::Protocol(1002)),
            OP_TEXT | OP_CONT => {}
            _ => return Err(ReadError::Protocol(1002)),
        }
        let add = usize::try_from(frame.len).unwrap_or(usize::MAX);
        if self.buf.len().saturating_add(add) > self.max_len {
            return Err(ReadError::Protocol(1009));
        }
        Ok(())
    }

    fn apply_frame(&mut self, fin: bool, opcode: u8, payload: Vec<u8>) -> Result<Option<Message>, ReadError> {
        match opcode {
            OP_CLOSE => Ok(Some(Message::Close)),
            OP_PING => Ok(Some(Message::Ping(payload))),
            OP_PONG => Ok(Some(Message::Pong(payload))),
            OP_TEXT | OP_CONT => {
                self.buf.extend_from_slice(&payload);
                self.in_text = !fin;
                if !fin {
                    return Ok(None);
                }
                let raw = std::mem::take(&mut self.buf);
                let text = String::from_utf8(raw).map_err(|_| ReadError::Protocol(1007))?;
                if text.contains('\n') {
                    return Err(ReadError::Protocol(1008));
                }
                Ok(Some(Message::Text(text)))
            }
            _ => Err(ReadError::Protocol(1002)),
        }
    }
}

struct FrameHeader {
    fin: bool,
    rsv: u8,
    opcode: u8,
    masked: bool,
    len: u64,
    mask: [u8; 4],
}

fn read_frame_header<R: Read>(r: &mut R) -> Result<Option<FrameHeader>, ReadError> {
    let mut b0 = [0u8; 1];
    if r.read(&mut b0)? == 0 {
        return Ok(None);
    }
    let mut b1 = [0u8; 1];
    r.read_exact(&mut b1)?;
    let masked = b1[0] & 0x80 != 0;
    let len = read_length(r, b1[0] & 0x7F)?;
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    Ok(Some(FrameHeader { fin: b0[0] & 0x80 != 0, rsv: b0[0] & 0x70, opcode: b0[0] & 0x0F, masked, len, mask }))
}

fn read_length<R: Read>(r: &mut R, code: u8) -> Result<u64, ReadError> {
    match code {
        0..=125 => Ok(u64::from(code)),
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            Ok(u64::from(u16::from_be_bytes(b)))
        }
        _ => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            let len = u64::from_be_bytes(b);
            if len >> 63 != 0 { Err(ReadError::Protocol(1002)) } else { Ok(len) }
        }
    }
}

fn read_masked_payload<R: Read>(r: &mut R, len: u64, mask: [u8; 4]) -> Result<Vec<u8>, ReadError> {
    let size = usize::try_from(len).map_err(|_| ReadError::Protocol(1009))?;
    let mut payload = vec![0u8; size];
    r.read_exact(&mut payload)?;
    for (i, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[i & 3];
    }
    Ok(payload)
}

/// Writes one unmasked server-to-client text frame.
pub fn write_text(w: &mut impl Write, text: &str) -> io::Result<()> {
    write_frame(w, OP_TEXT, text.as_bytes())
}

/// Writes one unmasked server-to-client Pong frame.
pub fn write_pong(w: &mut impl Write, data: &[u8]) -> io::Result<()> {
    write_frame(w, OP_PONG, &data[..data.len().min(125)])
}

/// Writes one unmasked server-to-client Close frame with a 16-bit status code.
pub fn write_close(w: &mut impl Write, code: u16) -> io::Result<()> {
    write_frame(w, OP_CLOSE, &code.to_be_bytes())
}

fn write_frame(w: &mut impl Write, opcode: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | opcode);
    let len = payload.len();
    if len <= 125 {
        head.push(len as u8);
    } else if let Ok(short) = u16::try_from(len) {
        head.push(126);
        head.extend_from_slice(&short.to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&(len as u64).to_be_bytes());
    }
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn client_frame(fin: bool, opcode: u8, masked: bool, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(if fin { 0x80 } else { 0 } | opcode);
        let mask_bit = if masked { 0x80 } else { 0 };
        if payload.len() <= 125 {
            out.push(mask_bit | payload.len() as u8);
        } else {
            out.push(mask_bit | 126);
            out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        if masked {
            let mask = [0x37, 0xFA, 0x21, 0x3D];
            out.extend_from_slice(&mask);
            for (i, &b) in payload.iter().enumerate() {
                out.push(b ^ mask[i & 3]);
            }
        } else {
            out.extend_from_slice(payload);
        }
        out
    }

    #[test]
    fn accept_key_matches_rfc6455_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn seeded_fragmented_text_obeys_the_assembled_byte_limit() {
        for seed in 0..128 {
            eprintln!("fragment seed {seed}");
            let mut rng = fastrand::Rng::with_seed(seed);
            let text: String = (0..rng.usize(1..160)).map(|_| rng.choice(['a', ' ', '雪', '🙂']).unwrap()).collect();
            let split = rng.usize(0..=text.len());
            let (first, last) = text.as_bytes().split_at(split);
            let ping = rng.u32(..).to_le_bytes();
            let mut wire = client_frame(false, OP_TEXT, true, first);
            wire.extend(client_frame(true, OP_PING, true, &ping));
            wire.extend(client_frame(true, OP_CONT, true, last));
            let mut stream = wire.as_slice();
            let mut reader = MessageReader::new(text.len());
            assert_eq!(reader.read(&mut stream).unwrap(), Some(Message::Ping(ping.to_vec())), "seed {seed}");
            assert_eq!(reader.read(&mut stream).unwrap(), Some(Message::Text(text.clone())), "seed {seed}");
            assert_eq!(reader.read(&mut stream).unwrap(), None, "seed {seed}");

            let mut stream = wire.as_slice();
            let mut reader = MessageReader::new(text.len() - 1);
            let mut result = reader.read(&mut stream);
            if matches!(result, Ok(Some(Message::Ping(_)))) {
                result = reader.read(&mut stream);
            }
            assert!(matches!(result, Err(ReadError::Protocol(1009))), "seed {seed}");
        }
    }

    #[test]
    fn seeded_wire_bytes_and_partial_frames_stay_bounded() {
        for seed in 0..256 {
            eprintln!("frame seed {seed}");
            let mut rng = fastrand::Rng::with_seed(seed);
            let cap = rng.usize(1..=256);
            let mut bytes = vec![0; rng.usize(0..=256)];
            rng.fill(&mut bytes);
            let mut stream = bytes.as_slice();
            let mut reader = MessageReader::new(cap);
            while let Ok(Some(message)) = reader.read(&mut stream) {
                match message {
                    Message::Text(s) => assert!(s.len() <= cap && !s.contains('\n'), "seed {seed}"),
                    Message::Ping(p) | Message::Pong(p) => assert!(p.len() <= 125, "seed {seed}"),
                    Message::Close => break,
                }
            }

            let frame = client_frame(true, OP_TEXT, true, &bytes);
            let cut = rng.usize(1..frame.len());
            let result = MessageReader::new(bytes.len()).read(&mut &frame[..cut]);
            assert!(
                matches!(result, Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof),
                "seed {seed}, cut {cut}"
            );
        }
    }

    #[test]
    fn reads_masked_text_and_rejects_unmasked() {
        let good = client_frame(true, OP_TEXT, true, br#"{"id":1}"#);
        let mut r = MessageReader::new(1024);
        assert_eq!(r.read(&mut Cursor::new(good)).unwrap(), Some(Message::Text(r#"{"id":1}"#.into())));

        let unmasked = client_frame(true, OP_TEXT, false, b"hi");
        assert!(matches!(r.read(&mut Cursor::new(unmasked)), Err(ReadError::Protocol(1002))));
    }

    #[test]
    fn assembles_continuation_frames_with_interleaved_ping() {
        let mut wire = client_frame(false, OP_TEXT, true, b"{\"a\":");
        wire.extend(client_frame(true, OP_PING, true, b"ok"));
        wire.extend(client_frame(true, OP_CONT, true, b"1}"));
        let mut cur = Cursor::new(wire);
        let mut r = MessageReader::new(1024);
        assert_eq!(r.read(&mut cur).unwrap(), Some(Message::Ping(b"ok".to_vec())));
        assert_eq!(r.read(&mut cur).unwrap(), Some(Message::Text("{\"a\":1}".into())));
        assert_eq!(r.read(&mut cur).unwrap(), None);
    }

    #[test]
    fn rejects_over_cap_newline_and_binary_frames() {
        let big = client_frame(true, OP_TEXT, true, &[b'x'; 20]);
        assert!(matches!(MessageReader::new(16).read(&mut Cursor::new(big)), Err(ReadError::Protocol(1009))));

        let newline = client_frame(true, OP_TEXT, true, b"a\nb");
        assert!(matches!(MessageReader::new(64).read(&mut Cursor::new(newline)), Err(ReadError::Protocol(1008))));

        let binary = client_frame(true, OP_BINARY, true, b"abc");
        assert!(matches!(MessageReader::new(64).read(&mut Cursor::new(binary)), Err(ReadError::Protocol(1003))));
    }

    #[test]
    fn writes_server_text_frame_for_small_and_medium_payloads() {
        let mut small = Vec::new();
        write_text(&mut small, "hi").unwrap();
        assert_eq!(small, vec![0x81, 2, b'h', b'i']);

        let body = "a".repeat(200);
        let mut med = Vec::new();
        write_text(&mut med, &body).unwrap();
        assert_eq!(&med[..4], &[0x81, 126, 0, 200]);
        assert_eq!(&med[4..], body.as_bytes());
    }
}
