//! Builds, the pin and reload (1.8): `builds/<commit>/` with its manifest, `pin` and
//! `pins.log`, and the end of a reload at start.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::jobs::launch;
use crate::paths::Home;
use crate::{clock, files};

pub mod changelog;
pub mod material;
mod pins;
mod shortcut;
pub mod skills;

/// What went wrong with a build, a pin or a reload.
#[derive(Debug)]
pub enum BuildError {
    Git(crate::git::GitError),
    /// A file of the build or of the pin: what was being done, and the error.
    Io {
        what: String,
        source: std::io::Error,
    },
    /// A reload or a pin the checks refuse (1.8.6.1); a rollback names the state's backups (1.8.8).
    Refused {
        why: mork_core::maintenance::reload::Refusal,
        backups: Option<String>,
    },
    /// The build, or what it left, is wrong: why.
    Bad(String),
    /// Mork project isn't configured.
    NoProject(String),
    /// No self-build project is configured (`[mork] project` is unset).
    NoSelfBuild,
    /// `pin.next` doesn't fit this start (1.8.6.6).
    Reload(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git(e) => write!(f, "{e}"),
            Self::Io { what, source } => write!(f, "{what}: {source}"),
            Self::Refused { why, backups: Some(b) } => write!(f, "{why}; the state's backups: {b}"),
            Self::Refused { why, backups: None } => write!(f, "{why}"),
            Self::Bad(why) | Self::Reload(why) => write!(f, "{why}"),
            Self::NoProject(p) => write!(f, "[mork] project names `{p}`, which isn't configured"),
            Self::NoSelfBuild => write!(f, "no self-build project is configured (`[mork] project` is unset)"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<crate::git::GitError> for BuildError {
    fn from(e: crate::git::GitError) -> Self {
        Self::Git(e)
    }
}

impl From<BuildError> for String {
    fn from(e: BuildError) -> Self {
        e.to_string()
    }
}

fn io(what: impl Into<String>) -> impl FnOnce(std::io::Error) -> BuildError {
    let what = what.into();
    move |source| BuildError::Io { what, source }
}

pub use pins::{
    PinNext, ReloadNote, check_reload_target, finish_reload, moved_to_since, pin, reconcile, reload_said, write_pin,
};
pub use shortcut::{check_shortcut, link_path};

pub const MANIFEST_FORMAT: u32 = 1;

/// `manifest.json` (1.8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub commit: String,
    pub state_oldest: u32,
    pub state_newest: u32,
    pub protocol: u32,
    pub job_formats: Vec<u32>,
    pub reply_schemas: Vec<u32>,
    pub built_ms: i64,
}

impl Manifest {
    pub fn ranges(&self) -> mork_core::maintenance::reload::Manifest {
        mork_core::maintenance::reload::Manifest {
            state_oldest: self.state_oldest,
            state_newest: self.state_newest,
            job_formats: self.job_formats.clone(),
            reply_schemas: self.reply_schemas.clone(),
        }
    }
}

pub fn binary(home: &Home, commit: &str) -> PathBuf {
    home.builds().join(commit).join("mork")
}

/// A build's manifest, when the build exists: a full commit's folder, its binary a file.
pub fn manifest(home: &Home, commit: &str) -> Option<Manifest> {
    let full = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
    if !full || !std::fs::symlink_metadata(binary(home, commit)).is_ok_and(|m| m.is_file()) {
        return None;
    }
    files::read_json(&home.builds().join(commit).join("manifest.json")).ok().flatten()
}

/// The complete builds (`builds/<commit>/` with its binary and a manifest of that commit) whose
/// manifests read a state version (2.3.2).
pub fn readers_of(home: &Home, version: u32) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(home.builds()) else { return Vec::new() };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            let m = manifest(home, &name).filter(|m| m.commit == name)?;
            (m.state_oldest <= version && version <= m.state_newest).then(|| m.commit.chars().take(7).collect())
        })
        .collect();
    out.sort();
    out
}

pub fn latest_backup(home: &Home) -> Option<String> {
    let rd = std::fs::read_dir(home.backups()).ok()?;
    rd.flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(t, p)| (*t, p.clone()))
        .map(|(_, p)| p.display().to_string())
}

// Read as itself: never through a symlink, and only a regular file (M1.1).
fn open_in_io(io_dir: &Path, name: &str) -> Result<std::fs::File, BuildError> {
    if std::fs::symlink_metadata(io_dir).map_or(true, |m| !m.is_dir()) {
        return Err(BuildError::Bad("the build's io folder isn't a folder".into()));
    }
    let f = mork_sys::open_no_follow(&io_dir.join(name)).map_err(io(format!("no {name} the build made")))?;
    if !f.metadata().is_ok_and(|m| m.is_file()) {
        return Err(BuildError::Bad(format!("the build's {name} isn't a file")));
    }
    Ok(f)
}

/// Puts a finished build from its job's `io/` in place, refusing one of another commit: via
/// `builds/.tmp-<c>`, renamed to `builds/<c>`, the source folder removed (1.8.3, step 5).
pub fn install(home: &Home, commit: &str, io_dir: &Path) -> Result<(), BuildError> {
    use std::io::Read;
    // Already in place (a crash after the rename, before the state knew): done, once.
    if manifest(home, commit).is_some_and(|m| m.commit == commit) {
        let _ = std::fs::remove_dir_all(home.builds().join("src").join(commit));
        return Ok(());
    }
    let mut text = String::new();
    open_in_io(io_dir, "manifest.json")?
        .take(1 << 20)
        .read_to_string(&mut text)
        .map_err(io("the manifest can't be read"))?;
    let v: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| BuildError::Bad(format!("the manifest isn't JSON: {e}")))?;
    let built = v["commit"].as_str().unwrap_or_default();
    if built != commit {
        return Err(BuildError::Bad(format!("the binary built reports commit `{built}`, not {commit}")));
    }
    let m = Manifest {
        format: MANIFEST_FORMAT,
        commit: commit.to_owned(),
        state_oldest: u32::try_from(v["state_oldest"].as_u64().unwrap_or(0)).unwrap_or(0),
        state_newest: u32::try_from(v["state_newest"].as_u64().unwrap_or(0)).unwrap_or(0),
        protocol: u32::try_from(v["protocol"].as_u64().unwrap_or(0)).unwrap_or(0),
        job_formats: list(&v["job_formats"]),
        reply_schemas: list(&v["reply_schemas"]),
        built_ms: clock::now_ms(),
    };
    let mut bin = open_in_io(io_dir, "mork")?;
    let tmp = home.builds().join(format!(".tmp-{commit}"));
    let _ = std::fs::remove_dir_all(&tmp);
    let at = tmp.display().to_string();
    std::fs::create_dir_all(&tmp).map_err(io(&at))?;
    let mut out = std::fs::File::create(tmp.join("mork")).map_err(io(&at))?;
    std::io::copy(&mut bin, &mut out).map_err(io("the binary can't be copied"))?;
    out.sync_all().map_err(io(&at))?;
    mork_sys::chmod(&tmp.join("mork"), 0o755).map_err(io(&at))?;
    files::write_json(&tmp.join("manifest.json"), &m).map_err(io(&at))?;
    std::fs::rename(&tmp, home.builds().join(commit)).map_err(io(&at))?;
    let _ = std::fs::remove_dir_all(home.builds().join("src").join(commit));
    Ok(())
}

fn list(v: &serde_json::Value) -> Vec<u32> {
    v.as_array().into_iter().flatten().filter_map(|x| u32::try_from(x.as_u64()?).ok()).collect()
}

/// Mork's own repo, as the operator's checkout has it, and its base (1.8.1).
pub fn mork_repo(config: &mork_types::config::Config) -> Result<(PathBuf, String), BuildError> {
    let id = config.mork.project.as_ref().ok_or(BuildError::NoSelfBuild)?;
    let p = config.projects.get(id).ok_or_else(|| BuildError::NoProject(id.to_string()))?;
    Ok((PathBuf::from(&p.repo), p.base.clone()))
}

/// What a build job is made of: its full commit and its spec (1.8.3, steps 1 to 3).
pub fn prepare(
    ctx: &crate::jobs::context::Context,
    commit: &str,
    now: i64,
) -> Result<mork_types::job::JobSpec, BuildError> {
    let home = &ctx.home;
    let id_project = ctx.config.mork.project.as_ref().ok_or(BuildError::NoSelfBuild)?;
    let (checkout, _) = mork_repo(&ctx.config)?;
    // Mork's mirror when it exists and holds the commit (a merge only Bitbucket made is
    // there, never in the checkout, M5.14); else the operator's checkout (1.8.3, step 2).
    let mirror = home.mirrors().join(id_project.as_str());
    let in_mirror = mirror.join("HEAD").exists().then(|| git::resolve(home, &mirror, commit).ok()).flatten();
    let (repo, full) = match in_mirror {
        Some(full) => (mirror, full),
        None => {
            let full = git::resolve(home, &checkout, commit)?;
            (checkout, full)
        }
    };
    let src = home.builds().join("src").join(&full);
    let target = home.builds().join("target");
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(&src).map_err(io(src.display().to_string()))?;
    std::fs::create_dir_all(&target).map_err(io(target.display().to_string()))?;
    let tar = home.builds().join("src").join(format!(".{full}.tar"));
    let tar_s = tar.display().to_string();
    git::archive(home, &repo, &full, &tar)?;
    // `-m`: files dated now, not at their commit. Cargo names a crate's artifacts the same in every folder, so in
    // the shared target an older commit time would keep the last build's crates.
    let out =
        launch::short(home.root(), &["tar", "-xmf", &tar_s], &src, Duration::from_secs(120)).map_err(io("tar"))?;
    let _ = std::fs::remove_file(&tar);
    if !out.status.success() {
        return Err(BuildError::Bad(format!("tar: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    let owner = mork_types::AgentKey::new("mork/build");
    let id = mork_core::state::names::job_id(now, &owner, crate::random::salt());
    let io = home.job(id.as_str()).join("io");
    // Mork's repo is a project like any other: its toolchains and ports (1.8.1).
    let project = Some(id_project.as_str());
    let toolchains = ctx.project_toolchains(project);
    let mut env = ctx.env(&id, &owner, None, &toolchains, &io);
    env.insert("MORK_BUILD_COMMIT".into(), full.clone());
    let (t, i) = (quote(&target.display().to_string()), quote(&io.display().to_string()));
    // `sh` exports its own `PWD`; the build's processes get only the wall's (1.4.11.4). The binary is copied to
    // `io/` inside the wall and that copy prints the manifest: the shared target is never read outside (M1.1, M1.2).
    let script = format!(
        "env -u PWD cargo build --release --locked --target-dir {t} && cp {t}/release/mork {i}/mork && env -u PWD {i}/mork version --json > {i}/manifest.json"
    );
    let argv = vec!["/bin/sh".into(), "-c".into(), script];
    let places = ctx
        .places(&io, Vec::new(), vec![src.display().to_string(), target.display().to_string()], None, &toolchains)
        .map_err(BuildError::Bad)?;
    let src_s = src.display().to_string();
    Ok(ctx.spec(
        &id,
        mork_types::job::JobKind::Build,
        &owner,
        argv,
        &src_s,
        &places,
        env,
        Some(ctx.ports(project)),
        now,
    ))
}

fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

pub fn commit_of(spec: &mork_types::job::JobSpec) -> String {
    spec.env.get("MORK_BUILD_COMMIT").cloned().unwrap_or_default()
}

/// After a build job ended: put the build in place, or clean up and say why not.
pub fn finish(home: &Home, spec: &mork_types::job::JobSpec, exit_ok: bool, err: &str) -> Result<(), BuildError> {
    let commit = commit_of(spec);
    let io = home.job(spec.id.as_str()).join("io");
    let result = if exit_ok {
        install(home, &commit, &io)
    } else {
        Err(BuildError::Bad(format!("cargo failed: {}", err.trim().lines().last().unwrap_or("no output"))))
    };
    if result.is_err() {
        let _ = std::fs::remove_dir_all(home.builds().join("src").join(&commit));
        let _ = std::fs::remove_dir_all(home.builds().join(format!(".tmp-{commit}")));
    }
    result
}

/// What the unsettled jobs need a target to read: their formats, and their reply schemas'
/// versions (2.3.6).
pub fn unsettled(state: &mork_core::state::State) -> mork_core::maintenance::reload::Unsettled {
    let mut u = mork_core::maintenance::reload::Unsettled::default();
    for j in state.jobs.values().filter(|j| j.settled.is_none()) {
        u.formats.push(j.spec.format);
        if let Some(schema) = j.io_files.get("schema.json") {
            let v = serde_json::from_str(schema).unwrap_or_default();
            u.reply_schemas.push(mork_types::reply::version_of(&v));
        }
    }
    u
}

/// The checks a reload makes before anything changes (1.8.6.1), one function for the engine and
/// `mork pin` with none running (1.12.6); a rollback the state is too new for names its backups (1.8.8).
pub fn check_target(
    home: &Home,
    config: &mork_types::config::Config,
    commit: &str,
    state_version: u32,
    unsettled: &mork_core::maintenance::reload::Unsettled,
    under_way: bool,
) -> Result<String, BuildError> {
    use mork_core::maintenance::reload::Refusal;
    let (repo, base) = mork_repo(config)?;
    let full = git::resolve(home, &repo, commit).map_err(|e| BuildError::Refused {
        why: Refusal::NoBuild { short: format!("{commit}: {e}") },
        backups: None,
    })?;
    let target = mork_core::maintenance::reload::Target {
        manifest: manifest(home, &full).map(|m| m.ranges()),
        on_master: git::on_master(home, &repo, &base, &full) == Some(true),
    };
    match mork_core::maintenance::reload::refusal(&full, &target, state_version, unsettled, under_way) {
        Some(why @ Refusal::StateVersion { .. }) => Err(BuildError::Refused { why, backups: Some(backups(home)) }),
        Some(why) => Err(BuildError::Refused { why, backups: None }),
        None => Ok(full),
    }
}

fn backups(home: &Home) -> String {
    let Ok(rd) = std::fs::read_dir(home.backups()) else { return "none".into() };
    let mut all: Vec<_> = rd
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path().display().to_string())))
        .collect();
    all.sort_by(|a, b| b.cmp(a));
    if all.is_empty() { "none".into() } else { all.into_iter().map(|(_, p)| p).collect::<Vec<_>>().join(", ") }
}
