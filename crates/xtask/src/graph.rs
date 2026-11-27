//! T1.46 (and T1.1): the crate graph, from `cargo metadata`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

// The only crates `core` may reach, directly or not: `types` and pure serialization crates
// (T1.46: no async runtime, database, HTTP, file watching, system call or randomness crate).
const CORE_ALLOWED: &[&str] = &[
    "mork-types",
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "itoa",
    "ryu",
    "zmij",
    "memchr",
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

/// The allowed workspace edges of 1.1.2 (normal dependencies). A workspace crate missing here
/// may depend on no other.
pub fn allowed_edges() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let set = |xs: &[&'static str]| xs.iter().copied().collect::<BTreeSet<_>>();
    BTreeMap::from([
        ("mork-types", set(&[])),
        ("mork-sys", set(&[])),
        ("mork-core", set(&["mork-types"])),
        ("mork-engine", set(&["mork-core", "mork-types", "mork-sys"])),
        ("mork-app", set(&["mork-types", "mork-core", "mork-engine", "mork-sys"])),
        ("mork-fakes", set(&[])),
        ("xtask", set(&[])),
    ])
}

/// The workspace's own packages, by name: `workspace_members`, or failing that every package
/// named `mork-…` and `xtask`.
pub fn members(meta: &Value) -> BTreeSet<String> {
    match meta["workspace_members"].as_array() {
        Some(ids) => ids.iter().filter_map(Value::as_str).map(|id| name_of(meta, id)).collect(),
        None => meta["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["name"].as_str())
            .filter(|n| n.starts_with("mork-") || *n == "xtask")
            .map(str::to_owned)
            .collect(),
    }
}

pub fn metadata(root: &Path) -> Result<Value, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

fn name_of(meta: &Value, id: &str) -> String {
    meta["packages"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .and_then(|p| p["name"].as_str())
        .unwrap_or(id)
        .to_owned()
}

/// Each package's normal (non-dev, non-build) dependencies, by name, from the resolve graph.
pub fn normal_deps(meta: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for node in meta["resolve"]["nodes"].as_array().into_iter().flatten() {
        let name = name_of(meta, node["id"].as_str().unwrap_or(""));
        let deps = node["deps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null()))
            .map(|d| name_of(meta, d["pkg"].as_str().unwrap_or("")))
            .collect();
        out.insert(name, deps);
    }
    out
}

fn transitive(deps: &BTreeMap<String, BTreeSet<String>>, start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![start.to_owned()];
    while let Some(n) = todo.pop() {
        for d in deps.get(&n).into_iter().flatten() {
            if seen.insert(d.clone()) {
                todo.push(d.clone());
            }
        }
    }
    seen
}

/// Reports missing workspace crates, missing or unexpected dependency edges,
/// and dependencies outside the allowlist reachable from `mork-core`.
pub fn check_graph(meta: &Value) -> Vec<String> {
    let deps = normal_deps(meta);
    let ours = members(meta);
    let allowed = allowed_edges();
    let mut out = Vec::new();
    for pkg in &ours {
        let ok = allowed.get(pkg.as_str()).cloned().unwrap_or_default();
        for d in deps.get(pkg).into_iter().flatten().filter(|d| ours.contains(*d)) {
            if !ok.contains(d.as_str()) {
                out.push(format!("{pkg} depends on {d}, outside 1.1.2"));
            }
        }
    }
    for (pkg, want) in &allowed {
        if !ours.contains(*pkg) {
            if meta["workspace_members"].is_array() || pkg.starts_with("mork-") && deps.contains_key(*pkg) {
                out.push(format!("{pkg}: missing from the workspace"));
            }
            continue;
        }
        for a in want {
            if !deps.get(*pkg).is_some_and(|ds| ds.contains(*a)) {
                out.push(format!("{pkg} doesn't depend on {a}"));
            }
        }
    }
    for d in transitive(&deps, "mork-core") {
        if !CORE_ALLOWED.contains(&d.as_str()) {
            out.push(format!("mork-core reaches {d}"));
        }
    }
    out
}

/// Only `sys` lacks `#![forbid(unsafe_code)]`, at the root of every target of every
/// workspace crate: libraries, binaries and build scripts alike (M0.3).
pub fn check_forbid(meta: &Value, root: &Path) -> Vec<String> {
    let ours = members(meta);
    let mut out = Vec::new();
    for pkg in meta["packages"].as_array().into_iter().flatten() {
        let name = pkg["name"].as_str().unwrap_or_default();
        if !ours.contains(name) {
            continue;
        }
        for t in pkg["targets"].as_array().into_iter().flatten() {
            let kinds: Vec<&str> = t["kind"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !kinds.iter().any(|k| ["lib", "bin", "custom-build", "proc-macro", "rlib"].contains(k)) {
                continue;
            }
            let src = std::path::PathBuf::from(t["src_path"].as_str().unwrap_or_default());
            let text = std::fs::read_to_string(&src).unwrap_or_default();
            let has = text.lines().any(|l| l.trim() == "#![forbid(unsafe_code)]");
            let rel = crate::walk::rel(root, &src);
            if name == "mork-sys" && kinds.contains(&"lib") && has {
                out.push("crates/sys: forbids unsafe, but it is where unsafe lives".into());
            } else if name != "mork-sys" && !has {
                out.push(format!("{rel}: lacks #![forbid(unsafe_code)]"));
            }
        }
    }
    out
}
