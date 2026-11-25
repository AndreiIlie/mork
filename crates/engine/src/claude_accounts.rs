use std::collections::BTreeMap;
use std::fs::{self, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use factory_types::usage::Account;
use serde_json::Value;

use crate::{clock, random};

const SAVED: [(&str, &str); 3] =
    [("personal-max", "claude max"), ("work-team", "claude team"), ("small-team", "claude team small")];
const EVERY: Duration = Duration::from_secs(1);

/// The pool header for a known saved account.
pub(crate) fn header(name: &str) -> Option<&'static str> {
    SAVED.into_iter().find_map(|(saved, header)| (saved == name).then_some(header))
}

#[derive(Debug, Default)]
pub(crate) struct Accounts {
    pub live: Option<Account>,
    pub saved: BTreeMap<String, Account>,
    checked: Option<Instant>,
    stamps: Vec<Option<(u64, i64, i64)>>,
}

impl Accounts {
    pub fn read(home: &Path) -> Self {
        let mut accounts = Self::default();
        accounts.reload(home);
        accounts
    }

    pub fn refresh(&mut self, home: &Path) -> bool {
        if self.checked.is_some_and(|at| clock::elapsed(at) < EVERY) {
            return false;
        }
        self.checked = Some(clock::instant());
        if self.stamps == stamps(home) {
            return false;
        }
        let before = (self.live.clone(), self.saved.clone());
        self.reload(home);
        before != (self.live.clone(), self.saved.clone())
    }

    pub fn reload(&mut self, home: &Path) {
        self.checked = Some(clock::instant());
        self.stamps = stamps(home);
        self.saved = SAVED
            .into_iter()
            .filter_map(|(name, _)| {
                let login = Login::read(saved_path(home, name, "claude")).ok()?;
                Some((name.to_owned(), login.account()?))
            })
            .collect();
        self.live = Login::read(home.join(".claude.json")).ok().and_then(|login| login.account());
    }
}

fn stamps(home: &Path) -> Vec<Option<(u64, i64, i64)>> {
    std::iter::once(home.join(".claude.json"))
        .chain(SAVED.map(|(name, _)| saved_path(home, name, "claude")))
        .map(|path| {
            let metadata = fs::metadata(path).ok()?;
            Some((metadata.ino(), metadata.ctime(), metadata.ctime_nsec()))
        })
        .collect()
}

fn saved_path(home: &Path, name: &str, kind: &str) -> PathBuf {
    home.join(".claude/accounts").join(format!("{name}.{kind}.json"))
}

pub(crate) struct Switch {
    pub account: String,
    pub replaced: String,
    files: Vec<(WholeFile, Vec<u8>)>,
}

impl Switch {
    pub fn prepare(home: &Path, name: &str) -> Result<Self, String> {
        if header(name).is_none() {
            return Err(format!("no saved Claude account `{name}`"));
        }
        let home_login = Login::read(home.join(".claude.json"))?;
        let mut saved: BTreeMap<_, _> = SAVED
            .into_iter()
            .filter(|(saved, _)| *saved != name)
            .filter_map(|(name, _)| Login::read(saved_path(home, name, "claude")).ok().map(|login| (name, login)))
            .collect();
        saved.insert(name, Login::read(saved_path(home, name, "claude"))?);
        let current = home_login.account().ok_or("the home's Claude login matches no saved account")?;
        let from = saved
            .iter()
            .find(|(_, login)| login.account().is_some_and(|account| account.id == current.id))
            .map(|(name, _)| *name)
            .ok_or("the home's Claude login matches no saved account")?;
        if from == name {
            return Err(format!("the shared home is already on `{name}`"));
        }
        let old = saved.remove(from).ok_or("the current saved login is missing")?;
        let target = saved.remove(name).ok_or("the target saved login is missing")?;
        if target.account().is_none() {
            return Err(format!("the saved account `{name}` has no Claude login"));
        }
        let home_credentials = WholeFile::read(home.join(".claude/.credentials.json"))?;
        let old_credentials = WholeFile::read(saved_path(home, from, "credentials"))?;
        let target_credentials = WholeFile::read(saved_path(home, name, "credentials"))?;
        let old_json = old.with_login(&home_login.json)?;
        let home_json = home_login.with_login(&target.json)?;
        let saved_credentials = home_credentials.bytes.clone();
        Ok(Self {
            account: name.to_owned(),
            replaced: from.to_owned(),
            files: vec![
                (old_credentials, saved_credentials),
                (old.file, old_json),
                (home_credentials, target_credentials.bytes),
                (home_login.file, home_json),
            ],
        })
    }

    pub fn apply(&self) -> Result<(), String> {
        let staged = self.files.iter().map(|(file, bytes)| file.stage(bytes)).collect::<Result<Vec<_>, _>>()?;
        for file in staged {
            fs::rename(&file.temporary, &file.destination)
                .map_err(|e| format!("could not replace {}: {e}", file.destination.display()))?;
        }
        Ok(())
    }
}

struct Login {
    file: WholeFile,
    json: Value,
}

impl Login {
    fn read(path: PathBuf) -> Result<Self, String> {
        let file = WholeFile::read(path)?;
        let json = serde_json::from_slice::<Value>(&file.bytes)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("{} is not a JSON object", file.path.display()))?;
        Ok(Self { file, json })
    }

    fn account(&self) -> Option<Account> {
        crate::cli::limits::claude_account(&self.json)
    }

    fn with_login(&self, login: &Value) -> Result<Vec<u8>, String> {
        let mut json = self.json.clone();
        let object = json.as_object_mut().ok_or("the Claude configuration is not an object")?;
        for field in ["oauthAccount", "userID"] {
            match login.get(field) {
                Some(value) => {
                    object.insert(field.to_owned(), value.clone());
                }
                None => {
                    object.remove(field);
                }
            }
        }
        serde_json::to_vec_pretty(&json).map_err(|_| "could not encode the Claude configuration".to_owned())
    }
}

struct WholeFile {
    path: PathBuf,
    bytes: Vec<u8>,
    permissions: Permissions,
}

impl WholeFile {
    fn read(path: PathBuf) -> Result<Self, String> {
        let read = || -> std::io::Result<_> {
            let mut file = fs::File::open(&path)?;
            let permissions = file.metadata()?.permissions();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok((bytes, permissions))
        };
        let (bytes, permissions) = read().map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Ok(Self { path, bytes, permissions })
    }

    fn stage(&self, bytes: &[u8]) -> Result<Staged, String> {
        let temporary = self.path.with_extension(format!("{}.tmp", random::uuid()));
        // Login bytes must never pass through a temporary file with the default public mode.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| format!("could not prepare {}: {e}", self.path.display()))?;
        let staged = Staged { temporary, destination: self.path.clone() };
        let mut write = || -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.set_permissions(self.permissions.clone())?;
            file.sync_all()
        };
        write().map_err(|e| format!("could not write {}: {e}", self.path.display()))?;
        Ok(staged)
    }
}

struct Staged {
    temporary: PathBuf,
    destination: PathBuf,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}
