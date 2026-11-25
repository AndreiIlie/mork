//! The engine's randomness (`core` has none, 1.12.1): job id salts and Claude session ids.

use std::io::Read;

fn bytes<const N: usize>() -> [u8; N] {
    bytes_exact::<N>().unwrap_or([0u8; N])
}

/// Reads `N` random bytes from `/dev/urandom`, propagating any open or read error (fac-671).
pub fn bytes_exact<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b)
}

/// Four hex digits' worth.
pub fn salt() -> u16 {
    u16::from_le_bytes(bytes::<2>())
}

/// A version-4 UUID, as Claude's `--session-id` takes it.
pub fn uuid() -> String {
    let mut b = bytes::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
