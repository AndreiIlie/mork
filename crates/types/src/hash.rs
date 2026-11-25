//! A stable, short hash of a text, for natural keys (2.5.4): FNV-1a, 64 bits, in hex. It needs
//! no crate and never changes between builds.

/// FNV-1a of the text's bytes, as 16 hex digits.
pub fn short(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}
