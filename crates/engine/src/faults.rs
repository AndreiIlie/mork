//! Settings reserved for builds with the `faults` feature.

/// Whether the test settings are honoured: only a build with `faults` reads them (1.10.6).
pub const TEST_SETTINGS: bool = cfg!(feature = "faults");

/// A test setting, or `None` in a production build.
pub fn test_env(name: &str) -> Option<String> {
    if TEST_SETTINGS { std::env::var(name).ok().filter(|v| !v.is_empty()) } else { None }
}
