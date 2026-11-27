//! Local project registration, before an engine or its data folders need to exist.

mod detect;
mod text;

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mork_types::config::{BoardConfig, Config};

use crate::jobs::launch;
use crate::paths::Home;
use crate::{config, files, seat_homes};

#[derive(Default)]
pub struct Options<'a> {
    pub repo: Option<&'a str>,
    pub name: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub test: Option<&'a str>,
}

fn at(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", path.display())
}

fn optional<T>(result: io::Result<T>, path: &Path) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(at(path, e)),
    }
}

fn resolved(path: &Path) -> Result<PathBuf, String> {
    if let Some(real) = optional(path.canonicalize(), path)? {
        return Ok(real);
    }
    if optional(fs::symlink_metadata(path), path)?.is_some() {
        return Err(at(path, "dangling link"));
    }
    let abs = std::path::absolute(path).map_err(|e| at(path, e))?;
    let parent = abs.parent().ok_or_else(|| at(path, "missing parent"))?;
    let name = abs.file_name().ok_or_else(|| at(path, "unresolved path"))?;
    Ok(resolved(parent)?.join(name))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut argv = vec!["git"];
    argv.extend_from_slice(args);
    let result =
        launch::short_unlogged(&argv, dir, Duration::from_secs(20), &[("GIT_TERMINAL_PROMPT".into(), "0".into())])
            .map_err(|e| at(dir, e))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    let line = String::from_utf8(result.stdout).map_err(|_| at(dir, "git returned a non-UTF-8 value"))?;
    Ok(line.strip_suffix('\n').unwrap_or(&line).to_owned())
}

struct Existing {
    text: String,
    config: Config,
}

impl Existing {
    fn read(path: &Path) -> Result<Option<Self>, String> {
        let Some(text) = optional(fs::read_to_string(path), path)? else { return Ok(None) };
        let config = config::parse(&text).map_err(|e| at(path, e))?;
        Ok(Some(Self { text, config }))
    }

    fn registered(&self, repo: &Path) -> Option<String> {
        self.config
            .projects
            .iter()
            .find(|(_, p)| Path::new(&p.repo).canonicalize().is_ok_and(|path| path == repo))
            .map(|(name, _)| format!("{} is already registered as {name}", repo.display()))
    }

    fn check_prefix(&self, prefix: &str) -> Result<(), String> {
        for (name, project) in &self.config.projects {
            let BoardConfig::Markdown { path } = &project.board;
            let path = Path::new(path).join("backlog/config.yml");
            if let Some(contents) = optional(fs::read_to_string(&path), &path)? {
                let (used, _) = mork_core::board::config(&contents).map_err(|e| at(&path, e))?;
                if prefix.eq_ignore_ascii_case(&used) {
                    return Err(format!("prefix {prefix} belongs to {name}: pass --prefix with unused letters"));
                }
            }
        }
        Ok(())
    }
}

struct Setup {
    tools: detect::Tools,
    assets: Option<PathBuf>,
    author: String,
}

impl Setup {
    fn detect(repo: &Path, bin: &Path) -> Result<Self, String> {
        let host = std::env::var_os("HOME").ok_or("set HOME to detect installed tools")?;
        let host = Path::new(&host).canonicalize().map_err(|e| at(Path::new(&host), e))?;
        let tools = detect::installed(&host)?;
        let identity = |key| {
            git(repo, &["config", "--get", key])
                .ok()
                .filter(|s| !s.trim().is_empty())
                .ok_or("set git config user.name and git config user.email before running init")
        };
        let author = format!("{} <{}>", identity("user.name")?, identity("user.email")?);
        Ok(Self { tools, assets: detect::web_assets(bin), author })
    }

    fn check_links(&self, tools: &Path) -> Result<(), String> {
        for (name, target) in &self.tools.links {
            let link = tools.join("bin").join(name);
            if optional(fs::symlink_metadata(&link), &link)?.is_some()
                && !fs::read_link(&link).is_ok_and(|path| path == *target)
            {
                return Err(at(&link, "already exists with a different target"));
            }
        }
        Ok(())
    }

    fn links(&self, home: &Home) -> Result<(), String> {
        if !self.tools.links.is_empty() {
            let rel = Path::new("tools/bin");
            seat_homes::mkdir(home.root(), rel).map_err(|e| at(&home.tools(), e))?;
            for (name, target) in &self.tools.links {
                mork_sys::link_beneath(home.root(), &rel.join(name), target)
                    .map_err(|e| at(&home.tools().join("bin").join(name), e))?;
            }
        }
        Ok(())
    }
}

fn board_prefix(home: &Home, file: &Path, name: &str, given: Option<&str>) -> Result<(String, bool), String> {
    let path = home.root().join(file);
    let Some(mut f) = optional(mork_sys::open_beneath(home.root(), file), &path)? else {
        return Ok((text::prefix(name, given)?, true));
    };
    let mut content = String::new();
    f.read_to_string(&mut content).map_err(|e| at(&path, e))?;
    let (prefix, _) = mork_core::board::config(&content).map_err(|e| at(&path, e))?;
    if !text::valid_prefix(&prefix) {
        return Err(at(&path, "task_prefix must contain only ASCII letters"));
    }
    if given.is_some_and(|s| !s.eq_ignore_ascii_case(&prefix)) {
        return Err(at(&path, format!("task_prefix is {prefix}, different from --prefix")));
    }
    Ok((prefix, false))
}

fn write_board(home: &Home, backlog: &Path, name: &str, prefix: &str, new: bool) -> Result<(), String> {
    for dir in ["tasks", "completed", "archive/tasks"] {
        let rel = backlog.join(dir);
        seat_homes::mkdir(home.root(), &rel).map_err(|e| at(&home.root().join(rel), e))?;
    }
    if new {
        let file = backlog.join("config.yml");
        mork_sys::write_beneath(home.root(), &file, text::board(name, prefix).as_bytes())
            .map_err(|e| at(&home.root().join(file), e))?;
    }
    Ok(())
}

fn directory(path: &Path) -> Result<File, String> {
    fs::create_dir_all(path).and_then(|()| File::open(path)).map_err(|e| at(path, e))
}

fn lock(file: &File) -> Result<(), String> {
    if mork_sys::try_lock_exclusive(file).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err("another mork init is running".into())
    }
}

fn locks(home: &Path, config: &Path) -> Result<(File, File), String> {
    let home = directory(home)?;
    lock(&home)?;
    let parent = directory(config.parent().ok_or("config path has no parent")?)?;
    let id = |f: &File| f.metadata().map(|m| (m.dev(), m.ino())).map_err(|e| e.to_string());
    if id(&parent)? != id(&home)? {
        lock(&parent)?;
    }
    Ok((home, parent))
}

struct Registration {
    home: Home,
    config: PathBuf,
    root: PathBuf,
    name: String,
    base: String,
    test: String,
    domains: Vec<&'static str>,
    install: Option<&'static str>,
}

impl Registration {
    fn prepare(home: Home, config: PathBuf, root: PathBuf, opts: &Options<'_>) -> Result<Self, String> {
        let branch =
            git(&root, &["symbolic-ref", "--quiet", "HEAD"]).map_err(|_| "check out a branch: HEAD is detached")?;
        let base = branch.strip_prefix("refs/heads/").ok_or("HEAD does not name a branch")?.to_owned();
        git(&root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).map_err(|_| "the branch needs a commit")?;
        let name = text::name(&root, opts.name)?;
        if opts.prefix.is_some_and(|s| !text::valid_prefix(s)) {
            return Err("pass --prefix with one or more ASCII letters".into());
        }
        let board = home.boards().join(&name).join("backlog");
        text::destinations_outside(&root, &[&config, &home.tools().join("bin"), &board])?;
        for entry in ["tasks", "completed", "archive/tasks", "config.yml"] {
            text::destinations_outside(&root, &[&board.join(entry)])?;
        }
        let cargo = root.join("Cargo.toml").is_file();
        let package_path = root.join("package.json");
        let package = optional(fs::read_to_string(&package_path), &package_path)?;
        let test = text::test_command(opts.test, cargo, package.as_deref())?.to_owned();
        let install = root.join("package-lock.json").is_file().then_some("npm ci");
        let mut domains = Vec::new();
        if cargo {
            domains.extend(["index.crates.io", "static.crates.io"]);
        }
        if package.is_some() {
            domains.push("registry.npmjs.org");
        }
        Ok(Self { home, config, root, name, base, test, domains, install })
    }

    fn save(&self, prior: Option<&Existing>, setup: Option<&Setup>, given: Option<&str>) -> Result<String, String> {
        let name = &self.name;
        if prior.is_some_and(|old| old.config.projects.contains_key(name.as_str())) {
            return Err(format!("project {name} is taken: pass --name with an unused project name"));
        }
        let board = self.home.boards().join(name);
        let backlog = Path::new("boards").join(name).join("backlog");
        let (prefix, new) = board_prefix(&self.home, &backlog.join("config.yml"), name, given)?;
        let start = match (prior, setup) {
            (Some(old), _) => {
                old.check_prefix(&prefix)?;
                old.text.clone()
            }
            (_, Some(setup)) => {
                setup.check_links(&self.home.tools())?;
                text::global(&setup.tools, &self.home.tools(), setup.assets.as_deref(), &setup.author)?
            }
            _ => return Err("the configuration disappeared: run mork init again".into()),
        };
        let project = text::Project {
            repo: &self.root,
            base: &self.base,
            test: &self.test,
            install: self.install,
            domains: self.domains.clone(),
        };
        let full = text::add_project(&start, name, &board, &project)?;
        let parsed = config::parse(&full)?;
        let permissions = if prior.is_some() {
            Some(fs::metadata(&self.config).map_err(|e| at(&self.config, e))?.permissions())
        } else {
            None
        };
        write_board(&self.home, &backlog, name, &prefix, new)?;
        if let Some(setup) = setup {
            setup.links(&self.home)?;
        }
        match permissions {
            Some(p) => files::write_whole_preserving(&self.config, full.as_bytes(), p),
            None => files::write_whole(&self.config, full.as_bytes()),
        }
        .map_err(|e| at(&self.config, e))?;
        Ok(self.report(&parsed, setup, &board, &prefix))
    }

    fn report(&self, config: &Config, setup: Option<&Setup>, board: &Path, prefix: &str) -> String {
        let mut lines = vec![
            format!("Config: {}", self.config.display()),
            format!("Project: {}", self.name),
            format!("Board: {} (prefix {prefix})", board.display()),
            format!("Base: {}", self.base),
            format!("Test: {}", self.test),
        ];
        if let Some(install) = self.install {
            lines.push(format!("Install: {install}"));
        }
        if let Some(setup) = setup {
            let clis = setup.tools.clis.iter().map(|cli| cli.as_str()).collect::<Vec<_>>();
            lines.push(format!("CLIs: {}", clis.join(", ")));
            lines.extend(config.mork.toolchains.iter().map(|t| format!("Toolchain: {}", t.path)));
            if setup.assets.is_none() {
                lines
                    .push("Set [mork] web_assets to the absolute path of the built web/dist for the dashboard.".into());
            }
            lines.extend(clis.iter().map(|cli| format!("mork login {cli}")));
        } else {
            lines.push("A running Mork reads the added project at its next start.".into());
        }
        lines.push("mork".into());
        lines.join("\n")
    }
}

/// Registers a checkout under a private markdown board and returns the terminal report.
pub fn register(home: &Home, opts: &Options<'_>, bin: &Path) -> Result<String, String> {
    let home = Home::new(resolved(home.root())?);
    let config = resolved(&config::path().ok_or("set MORK_CONFIG or HOME")?)?;
    let path = Path::new(opts.repo.unwrap_or("."));
    let path = path.canonicalize().map_err(|e| at(path, e))?;
    if path.starts_with(resolved(&home.clones())?) {
        return Err("cannot register a Mork agent clone".into());
    }
    let root =
        PathBuf::from(git(&path, &["rev-parse", "--show-toplevel"]).map_err(|_| at(&path, "not a git checkout"))?);
    let root = root.canonicalize().map_err(|e| at(&root, e))?;
    let existing = Existing::read(&config)?;
    if let Some(message) = existing.as_ref().and_then(|old| old.registered(&root)) {
        return Ok(message);
    }
    let registration = Registration::prepare(home, config, root, opts)?;
    let setup = if existing.is_none() { Some(Setup::detect(&registration.root, bin)?) } else { None };
    let _locks = locks(registration.home.root(), &registration.config)?;
    let current = Existing::read(&registration.config)?;
    if let Some(message) = current.as_ref().and_then(|old| old.registered(&registration.root)) {
        return Ok(message);
    }
    registration.save(current.as_ref(), setup.as_ref().filter(|_| current.is_none()), opts.prefix)
}
