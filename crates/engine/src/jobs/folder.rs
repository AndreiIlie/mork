//! The job folder (1.4.2): written whole by the engine, then the supervisor's records.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use mork_types::job::{Claim, ExitRecord, JobSpec, RunInfo};

use crate::files;

// A short alias lets test homes bind Unix sockets while their storage stays in the job (fac-65).
pub struct ShortTemp {
    path: PathBuf,
}

impl ShortTemp {
    pub fn new(target: &Path) -> io::Result<Self> {
        let target = fs::canonicalize(target)?;
        for _ in 0..16 {
            let path = PathBuf::from("/tmp").join(format!("mork-{}", crate::random::uuid()));
            match std::os::unix::fs::symlink(&target, &path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no unused temporary alias"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ShortTemp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The files of a job folder.
#[derive(Debug, Clone)]
pub struct JobDir {
    pub dir: PathBuf,
}

impl JobDir {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
    pub fn spec_path(&self) -> PathBuf {
        self.dir.join("job.json")
    }
    pub fn stdin(&self) -> PathBuf {
        self.dir.join("stdin")
    }
    pub fn io(&self) -> PathBuf {
        self.dir.join("io")
    }
    /// The folder its `spec`'s `MORK_IO` names: a seat's own for its turns (fac-654), else its `io/`.
    pub fn mork_io(&self, spec: &JobSpec) -> PathBuf {
        spec.env.get("MORK_IO").map_or_else(|| self.io(), PathBuf::from)
    }
    pub fn lock(&self) -> PathBuf {
        self.dir.join("lock")
    }
    pub fn claim_path(&self) -> PathBuf {
        self.dir.join("claim.json")
    }
    pub fn run_path(&self) -> PathBuf {
        self.dir.join("run.json")
    }
    pub fn out(&self) -> PathBuf {
        self.dir.join("out.jsonl")
    }
    /// When the engine read the stream's parts (fac-206).
    pub fn stamps(&self) -> PathBuf {
        self.dir.join("out.stamps")
    }
    pub fn err(&self) -> PathBuf {
        self.dir.join("err.log")
    }
    pub fn exit_path(&self) -> PathBuf {
        self.dir.join("exit.json")
    }
    pub fn sup_log(&self) -> PathBuf {
        self.dir.join("sup.log")
    }
    /// The messages delivered into a live turn, `<seq>.json` each, for its supervisor to feed it (fac-389).
    pub fn input(&self) -> PathBuf {
        self.dir.join("input")
    }
    /// An empty file, which the wall binds over a file its job must not read (fac-389).
    pub fn empty(&self) -> PathBuf {
        self.dir.join("empty")
    }

    pub fn exists(&self) -> bool {
        self.dir.join("job.json").exists()
    }
    pub fn spec(&self) -> io::Result<Option<JobSpec>> {
        files::read_json(&self.spec_path())
    }
    /// The account captured before this Claude job started; none for an older job or an unknown account.
    pub fn usage_account(&self) -> Option<mork_types::usage::Account> {
        files::read_json(&self.dir.join("usage-account.json")).ok().flatten()
    }

    /// Discards a running turn's attribution when a switch may change which login it uses.
    pub fn forget_usage_account(&self) -> io::Result<()> {
        match fs::remove_file(self.dir.join("usage-account.json")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
    pub fn claim(&self) -> io::Result<Option<Claim>> {
        files::read_json(&self.claim_path())
    }
    pub fn run(&self) -> io::Result<Option<RunInfo>> {
        files::read_json(&self.run_path())
    }
    pub fn exit(&self) -> io::Result<Option<ExitRecord>> {
        files::read_json(&self.exit_path())
    }

    /// Whether a supervisor holds the lock now.
    pub fn lock_held(&self) -> bool {
        match fs::File::open(self.lock()) {
            Ok(f) => matches!(mork_sys::lock_is_free(&f), Ok(false)),
            Err(_) => false,
        }
    }
}

/// What the engine writes before the supervisor starts.
#[derive(Debug, Clone)]
pub struct NewFolder<'a> {
    pub spec: &'a JobSpec,
    pub stdin: &'a [u8],
    /// Files put into `io/` (the reply's schema, images, bundles in).
    pub io_files: &'a [(String, Vec<u8>)],
}

/// Puts message `seq` in a live turn's `input/`, whole, for its supervisor to read (fac-389).
pub fn deliver(dir: &JobDir, seq: u64, delivery: &crate::cli::live::Delivery) -> io::Result<()> {
    files::write_json(&dir.input().join(format!("{seq}.json")), delivery)
}

/// Writes the folder under a temporary name and renames it into place whole (1.4.2.1).
/// A folder already in place is left as it is.
pub fn write(jobs_dir: &Path, new: &NewFolder<'_>) -> io::Result<JobDir> {
    let id = new.spec.id.as_str();
    let dest = jobs_dir.join(id);
    if dest.join("job.json").exists() {
        return Ok(JobDir::new(dest));
    }
    fs::create_dir_all(jobs_dir)?;
    let tmp = jobs_dir.join(format!(".tmp-{id}"));
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(tmp.join("io"))?;
    fs::create_dir_all(tmp.join("input"))?;
    files::write_json(&tmp.join("job.json"), new.spec)?;
    write_account(&tmp, new.spec)?;
    files::write_whole(&tmp.join("stdin"), new.stdin)?;
    files::write_whole(&tmp.join("empty"), b"")?;
    for (name, bytes) in new.io_files {
        let p = tmp.join("io").join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(p, bytes)?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(JobDir::new(dest))
}

/// Makes a seat's `io/` folder (fac-654) when missing, beneath the home's own `io/`.
pub fn make_seat_io(io_dir: &Path) -> io::Result<()> {
    let (Some(parent), Some(name)) = (io_dir.parent(), io_dir.file_name()) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no seat io/ folder"));
    };
    fs::create_dir_all(parent)?;
    mork_sys::mkdir_beneath(parent, Path::new(name))
}

/// Writes a file at `rel` in a seat's `io/` folder: the seat writes that folder between its turns, so each folder
/// and the file are made through no link it planted there (R9).
pub fn write_seat_file(io_dir: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let rel = Path::new(rel);
    let mut at = PathBuf::new();
    for part in rel.parent().into_iter().flat_map(Path::components) {
        at.push(part);
        mork_sys::mkdir_beneath(io_dir, &at)?;
    }
    mork_sys::write_beneath(io_dir, rel, bytes)
}

fn write_account(dir: &Path, spec: &JobSpec) -> io::Result<()> {
    if spec.argv.first().map(String::as_str) != Some("claude") {
        return Ok(());
    }
    let Some(home) = spec.env.get("HOME") else { return Ok(()) };
    let config = crate::login_files::regular(Path::new(home), Path::new(".claude.json"))
        .ok()
        .and_then(|file| serde_json::from_reader::<_, serde_json::Value>(file).ok());
    if let Some(account) = config.as_ref().and_then(crate::cli::limits::claude_account) {
        files::write_json(&dir.join("usage-account.json"), &account)?;
    }
    Ok(())
}
