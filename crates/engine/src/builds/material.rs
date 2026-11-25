//! Recover a digest-only start's manuals from the installed build that supplied them.

use std::time::Duration;

use factory_core::maintenance::material::Recorded;

use crate::jobs::launch;
use crate::paths::Home;

/// Fills missing baselines only when the recorded build's own manual matches its fingerprint.
pub fn recover(home: &Home, recorded: &mut Recorded) {
    let Some(build) = recorded.build.as_deref() else { return };
    if build.len() != 40 || !build.bytes().all(|b| b.is_ascii_hexdigit()) {
        return;
    }
    let binary = super::binary(home, build);
    if !binary.is_file() {
        return;
    }
    let current = factory_core::maintenance::manuals::digests();
    for key in current.keys().filter(|k| k.as_str() != "contract") {
        if recorded.digests.get(key) == current.get(key) || recorded.text(key).is_some() {
            continue;
        }
        let name = key.strip_prefix("role:").unwrap_or(key);
        let out = launch::short(
            home.root(),
            &[&binary.to_string_lossy(), "manual", name],
            home.root(),
            Duration::from_secs(5),
        );
        let Some(text) = out.ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8(o.stdout).ok()) else {
            continue;
        };
        if recorded.digests.get(key) == Some(&factory_types::hash::short(&text)) {
            recorded.texts.insert(key.clone(), text);
        }
    }
}
