//! The test home (1.10.7): each test makes its own `t-XXXXXX`; teardown, pass or fail, kills every process
//! naming it, removes it on success (a failure keeps it and prints the app log's tail); a leftover process fails.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct TestHome {
    pub root: PathBuf,
    keep: bool,
    // Set when a test means to leave nothing running and checks it itself.
    allow_leftovers: bool,
}

/// The names the fakes are linked under, and their binaries (1.10.1).
pub const FAKE_LINKS: &[(&str, &str)] = &[
    ("claude", "fake-claude"),
    ("codex", "fake-codex"),
    ("agy", "fake-agy"),
    ("cargo", "fake-cargo"),
    ("notify-send", "fake-notify-send"),
    ("fake-checks", "fake-checks"),
    ("ssh", "fake-ssh"),
    ("mysql", "fake-mysql"),
    ("mongoexport", "fake-mongoexport"),
];

fn suffix() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let mix = (std::process::id() ^ t.rotate_left(7) ^ n.wrapping_mul(0x9e37_79b9)) & 0x00ff_ffff;
    format!("{mix:06x}")
}

fn homes_dir() -> PathBuf {
    // Kept with xtask/src/hygiene.rs::homes_dir: the crate graph forbids xtask depending on fakes (fac-471).
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut hash = DefaultHasher::new();
    workspace.hash(&mut hash);
    PathBuf::from(std::env::var_os("HOME").unwrap()).join(format!(".t{:06x}", hash.finish() & 0x00ff_ffff))
}

impl TestHome {
    /// Makes `t-XXXXXX` in the workspace's short namespace beneath HOME, with `home/`, `tmp/`, `xdg/`,
    /// `fhome/` (Mork home), `cfg/` and `bin/` inside.
    pub fn new() -> Self {
        let keep = std::env::var("MORK_KEEP_TEST_HOMES").is_ok_and(|v| v == "1");
        Self::with_keep(keep)
    }

    /// As [`TestHome::new`], keeping the folder (not the processes) when `keep` is set.
    pub fn with_keep(keep: bool) -> Self {
        // The wall replaces /tmp; a fixture there cannot prove which other paths it hides (fac-471).
        let base = homes_dir();
        let root = loop {
            let candidate = base.join(format!("t-{}", suffix()));
            match std::fs::create_dir_all(&base).and_then(|()| std::fs::create_dir(&candidate)) {
                Ok(()) => break candidate,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotFound) => {}
                Err(e) => panic!("can't make a test home in {}: {e}", base.display()),
            }
        };
        for d in ["home", "tmp", "xdg", "fhome", "cfg", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap_or_else(|e| panic!("{d}: {e}"));
        }
        Self { root, keep, allow_leftovers: false }
    }

    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
    pub fn xdg(&self) -> PathBuf {
        self.root.join("xdg")
    }
    pub fn mork_home(&self) -> PathBuf {
        self.root.join("fhome")
    }
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("cfg")
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// The environment a process of this test runs with.
    pub fn env(&self) -> Vec<(String, String)> {
        let s = |p: PathBuf| p.display().to_string();
        vec![
            ("HOME".into(), s(self.home())),
            ("TMPDIR".into(), s(self.tmp())),
            ("XDG_RUNTIME_DIR".into(), s(self.xdg())),
            ("MORK_HOME".into(), s(self.mork_home())),
            ("MORK_CONFIG".into(), s(self.config_dir().join("mork.toml"))),
        ]
    }

    /// Links the fakes, built beside `mork_exe`, into `bin/` under their CLI names. The
    /// fakes are built first (once per test binary), so a test never runs a stale one.
    pub fn link_fakes(&self, mork_exe: &Path) {
        build_fakes_once();
        let dir = mork_exe.parent().unwrap_or(Path::new("."));
        for (link, bin) in FAKE_LINKS {
            let target = dir.join(bin);
            let _ = std::os::unix::fs::symlink(&target, self.bin().join(link));
        }
    }

    /// The test asserts on its own processes; the teardown still kills them.
    pub fn allow_leftovers(&mut self) {
        self.allow_leftovers = true;
    }

    /// Kills every process naming this home; returns how many there were.
    pub fn kill_processes(&self) -> usize {
        let needle = self.root.display().to_string();
        let found = naming(&needle);
        let count = found.len();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = naming(&needle);
            if left.is_empty() || Instant::now() >= deadline {
                break;
            }
            for p in left {
                kill(p);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        count
    }
}

impl Default for TestHome {
    fn default() -> Self {
        Self::new()
    }
}

static BUILT: std::sync::Once = std::sync::Once::new();

// Once per test binary: `cargo test -p mork-app` alone doesn't build another package's binaries.
fn build_fakes_once() {
    BUILT.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let ok = std::process::Command::new(cargo)
            .args(["build", "--quiet", "--bins", "--manifest-path", manifest])
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "couldn't build the fakes");
    });
}

fn kill(pid: i32) {
    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
}

/// Processes whose environment, working folder or command line holds `needle` or its resolved path.
pub fn naming(needle: &str) -> Vec<i32> {
    let me = std::process::id().cast_signed();
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    let resolved = std::fs::canonicalize(needle).ok().map(|p| p.as_os_str().as_encoded_bytes().to_vec());
    let needle = needle.as_bytes();
    rd.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&p| p != me && !zombie(p))
        .filter(|&p| {
            let has = |b: Vec<u8>| {
                std::iter::once(needle).chain(resolved.as_deref()).any(|n| b.windows(n.len()).any(|w| w == n))
            };
            std::fs::read(format!("/proc/{p}/environ")).is_ok_and(has)
                || std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(has)
                || std::fs::read_link(format!("/proc/{p}/cwd"))
                    .is_ok_and(|c| has(c.as_os_str().as_encoded_bytes().to_vec()))
        })
        .collect()
}

fn zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s.rsplit_once(')').map(|(_, r)| r.trim_start().starts_with('Z')))
        .unwrap_or(true)
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // The teardown always kills and checks; the flag keeps only the folder (M0.4).
        let named: Vec<String> = naming(&self.root.display().to_string())
            .into_iter()
            .map(|pid| {
                let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
                format!("{pid}: {}", String::from_utf8_lossy(&cmd).replace('\0', " ").trim())
            })
            .collect();
        let leftovers = self.kill_processes();
        let leaked = leftovers > 0 && !self.allow_leftovers;
        let failed = std::thread::panicking() || leaked;
        if failed {
            print_app_log(&self.mork_home().join("logs/app.log"));
        }
        if self.keep || failed {
            eprintln!("kept test home: {}", self.root.display());
        } else {
            // A process still ending can write into the folder while it is removed: kill and
            // remove again until it is gone.
            for _ in 0..10 {
                let _ = std::fs::remove_dir_all(&self.root);
                if !self.root.exists() {
                    break;
                }
                self.kill_processes();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
        if let Some(parent) = self.root.parent() {
            let _ = std::fs::remove_dir(parent);
        }
        if leaked && !std::thread::panicking() {
            panic!("the test left {leftovers} process(es) behind in {}: {named:?}", self.root.display());
        }
    }
}

fn print_app_log(path: &Path) {
    use std::io::BufRead;
    eprintln!("app log tail ({}):", path.display());
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("couldn't read app log: {e}");
            return;
        }
    };
    let mut tail = std::collections::VecDeque::with_capacity(60);
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        if tail.len() == 60 {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    for line in tail {
        eprintln!("{line}");
    }
}
