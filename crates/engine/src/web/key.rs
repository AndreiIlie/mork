//! The web bridge's owner-only secret key (fac-671): 32 random bytes stored as 64 ASCII hex digits.

use std::fs;
use std::io;
use std::path::Path;

/// Expected character length of the hex-encoded 32-byte key.
pub const KEY_HEX_LEN: usize = 64;

/// Whether `text` is a valid 64-character ASCII hex key.
pub fn is_valid(text: &str) -> bool {
    text.len() == KEY_HEX_LEN && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Reads and validates an existing key file, failing closed unless it is exactly 64 ASCII hex digits.
pub fn read(path: &Path) -> io::Result<String> {
    let raw = fs::read_to_string(path)?;
    if !is_valid(&raw) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("web key must be {KEY_HEX_LEN} ASCII hex digits"),
        ));
    }
    Ok(raw)
}

/// Loads `path`, or creates it atomically with mode 0600 when missing; malformed existing files fail closed.
pub fn load_or_create(path: &Path) -> io::Result<String> {
    match read(path) {
        Ok(key) => Ok(key),
        Err(e) if e.kind() == io::ErrorKind::NotFound => create(path),
        Err(e) => Err(e),
    }
}

fn create(path: &Path) -> io::Result<String> {
    let raw = crate::random::bytes_exact::<32>()?;
    if raw.iter().all(|&b| b == 0) {
        return Err(io::Error::other("random bytes for web key were all zeros"));
    }
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    crate::files::write_whole_mode(path, hex.as_bytes(), 0o600)?;
    Ok(hex)
}

/// Constant-time byte comparison for secret keys.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The dashboard address shared by browser launch and the printed key link.
pub fn address(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

/// The browser URL carrying the key in its fragment.
pub fn link(port: u16, key: &str) -> String {
    format!("{}#key={key}", address(port))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mork-web-key-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_validation_rejects_bad_inputs() {
        assert!(!is_valid(""));
        assert!(!is_valid(&"a".repeat(63)));
        assert!(!is_valid(&"a".repeat(65)));
        assert!(!is_valid(&format!("{}g", "a".repeat(63))));
        assert!(is_valid(&"a".repeat(64)));
        assert!(is_valid("0123456789abcdefABCDEF0123456789abcdefABCDEF0123456789abcdefABCD"));
    }

    #[test]
    fn load_or_create_makes_mode_0600_key_and_keeps_it() {
        let dir = scratch("create");
        let path = dir.join("web.key");
        let first = load_or_create(&path).unwrap();
        assert!(is_valid(&first));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let second = load_or_create(&path).unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_and_load_fail_closed_on_empty_or_malformed_file() {
        let dir = scratch("bad");
        let valid = "0".repeat(64);
        let with_nl = format!("{valid}\n");
        let with_space = format!(" {valid}");
        for (name, content) in [
            ("empty", ""),
            ("short", "abcd"),
            ("nonhex", &"z".repeat(64)),
            ("newline", &with_nl),
            ("space", &with_space),
        ] {
            let path = dir.join(name);
            fs::write(&path, content).unwrap();
            assert_eq!(read(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(load_or_create(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        let path = dir.join("exact");
        fs::write(&path, &valid).unwrap();
        assert_eq!(read(&path).unwrap(), valid);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn constant_time_eq_and_link() {
        assert!(constant_time_eq(b"abcdef", b"abcdef"));
        assert!(!constant_time_eq(b"abcdef", b"abcdeg"));
        assert!(!constant_time_eq(b"abcdef", b"abcde"));
        assert_eq!(link(41234, "deadbeef"), "http://127.0.0.1:41234/#key=deadbeef");
    }
}
