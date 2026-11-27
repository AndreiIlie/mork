//! The audits `cargo xtask check` runs (1.12.8): the crate graph (T1.46), the source audit
//! (T1.47), the caps (T1.48), one sentence per line (fac-645), and the hygiene step (T1.45).
#![forbid(unsafe_code)]

pub mod budget;
pub mod caps;
pub mod comments;
pub mod graph;
pub mod hygiene;
pub mod palette;
pub mod rust;
pub mod sentences;
pub mod source;
pub mod suite;
pub mod visibility;
pub mod walk;
pub mod walls;
pub mod web;
pub mod web_types;

use std::path::PathBuf;

/// The workspace's root folder, or the one `XTASK_ROOT` names (a test's staged tree).
pub fn root() -> PathBuf {
    if let Some(r) = std::env::var_os("XTASK_ROOT").filter(|r| !r.is_empty()) {
        return PathBuf::from(r);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or_else(|_| PathBuf::from("."))
}

/// The crates of 1.1.1, by folder name, and their package names.
pub const CRATES: &[(&str, &str)] = &[
    ("types", "mork-types"),
    ("core", "mork-core"),
    ("engine", "mork-engine"),
    ("ui", "mork-ui"),
    ("sys", "mork-sys"),
    ("app", "mork-app"),
    ("fakes", "mork-fakes"),
];
