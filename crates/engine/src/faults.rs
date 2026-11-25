//! Fault points (1.10.5, T6.3): `FACTORY_FAULT=<point>[@<n>]` SIGKILLs the process at the n-th hit;
//! `FACTORY_FAULT_LOG` and `FACTORY_FAULT_GATE` log and hold points. Without `faults`, nothing happens.

#[cfg(feature = "faults")]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static COUNTS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

    // One write per line: the processes that share the log never interleave within a line.
    fn log(line: &str) {
        if let Some(path) = std::env::var_os("FACTORY_FAULT_LOG") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    pub fn hit(point: &str) {
        log(&format!("{point}\n"));
        wait_for_gate(point);
        let Ok(spec) = std::env::var("FACTORY_FAULT") else { return };
        let (name, nth) = match spec.split_once('@') {
            Some((n, k)) => (n, k.parse::<u32>().unwrap_or(1)),
            None => (spec.as_str(), 1),
        };
        if name != point {
            return;
        }
        let count = {
            let Ok(mut guard) = COUNTS.lock() else { return };
            let map = guard.get_or_insert_with(HashMap::new);
            let c = map.entry(point.to_owned()).or_insert(0);
            *c += 1;
            *c
        };
        if count == nth {
            eprintln!("fault: {point}@{nth}");
            // The log says it fired, so a test knows the kill it asked for happened.
            log(&format!("fired {point}@{nth}\n"));
            // SIGKILL, as a crash anywhere is (T6.3): nothing of this process runs after it.
            let _ = factory_sys::kill(factory_sys::getpid(), 9);
            std::process::abort();
        }
    }

    fn wait_for_gate(point: &str) {
        let Some(spec) = super::test_env("FACTORY_FAULT_GATE") else { return };
        let Some((name, path)) = spec.split_once('=') else { return };
        if name != point {
            return;
        }
        while !std::path::Path::new(path).exists() {
            crate::clock::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(feature = "faults")]
pub use imp::hit;

#[cfg(not(feature = "faults"))]
pub fn hit(_point: &str) {}

/// Whether the test settings are honoured: only a build with `faults` reads them (1.10.6).
pub const TEST_SETTINGS: bool = cfg!(feature = "faults");

/// A test setting, or `None` in a production build.
pub fn test_env(name: &str) -> Option<String> {
    if TEST_SETTINGS { std::env::var(name).ok().filter(|v| !v.is_empty()) } else { None }
}
