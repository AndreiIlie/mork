use std::collections::BTreeMap;
use std::fs::{self, Permissions};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mork_types::usage::Account;
use serde_json::Value;

use crate::{
    clock,
    login_files::{self, Destination},
};

const EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub(crate) struct Accounts {
    pub live: Option<Account>,
    pub saved: BTreeMap<String, Account>,
    configured: BTreeMap<String, String>,
    checked: Option<Instant>,
    stamps: Vec<Option<(u64, i64, i64)>>,
}

impl Accounts {
    pub fn read(home: &Path, configured: BTreeMap<String, String>) -> Self {
        let mut accounts = Self { configured, ..Self::default() };
        accounts.reload(home);
        accounts
    }

    /// The pool header for a configured saved account.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.configured.get(name).map(String::as_str)
    }

    pub fn refresh(&mut self, home: &Path) -> bool {
        if self.checked.is_some_and(|at| clock::elapsed(at) < EVERY) {
            return false;
        }
        self.checked = Some(clock::instant());
        if self.stamps == stamps(home, &self.configured) {
            return false;
        }
        let before = (self.live.clone(), self.saved.clone());
        self.reload(home);
        before != (self.live.clone(), self.saved.clone())
    }

    pub fn reload(&mut self, home: &Path) {
        self.checked = Some(clock::instant());
        self.stamps = stamps(home, &self.configured);
        self.saved = self
            .configured
            .keys()
            .filter_map(|name| {
                let login = Login::read(saved_path(home, name, "claude")).ok()?;
                Some((name.clone(), login.account()?))
            })
            .collect();
        self.live = Login::read(home.join(".claude.json")).ok().and_then(|login| login.account());
    }
}

fn stamps(home: &Path, configured: &BTreeMap<String, String>) -> Vec<Option<(u64, i64, i64)>> {
    std::iter::once(home.join(".claude.json"))
        .chain(configured.keys().map(|name| saved_path(home, name, "claude")))
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
    pub fn prepare(
        home: &Path,
        logins: &Path,
        name: &str,
        configured: &BTreeMap<String, String>,
    ) -> Result<Self, String> {
        if !configured.contains_key(name) {
            return Err(format!("no saved Claude account `{name}`"));
        }
        let home_login = Login::read(home.join(".claude.json"))?;
        let mut saved: BTreeMap<&str, Login> = configured
            .keys()
            .map(String::as_str)
            .filter(|saved| *saved != name)
            .filter_map(|saved| Login::read(saved_path(home, saved, "claude")).ok().map(|login| (saved, login)))
            .collect();
        saved.insert(name, Login::read(saved_path(home, name, "claude"))?);
        let current = home_login.account().ok_or("the active Claude login matches no saved account")?;
        let from = saved
            .iter()
            .find(|(_, login)| login.account().is_some_and(|account| account.id == current.id))
            .map(|(name, _)| *name)
            .ok_or("the active Claude login matches no saved account")?;
        if from == name {
            return Err(format!("Claude account `{name}` is already active"));
        }
        let old = saved.remove(from).ok_or("the current saved login is missing")?;
        let target = saved.remove(name).ok_or("the target saved login is missing")?;
        if target.account().is_none() {
            return Err(format!("the saved account `{name}` has no Claude login"));
        }
        let credential = Path::new(login_files::CLAUDE);
        let active = Destination::new(logins, credential)
            .map_err(|e| format!("could not read {}: {e}", logins.join(credential).display()))?;
        let home_credentials = WholeFile::open(active)?;
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
        let staged = self
            .files
            .iter()
            .map(|(file, bytes)| {
                file.destination
                    .stage(bytes, Some(file.permissions.clone()))
                    .map_err(|e| format!("could not stage {}: {e}", file.destination.path.display()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for file in staged {
            file.commit().map_err(|e| e.to_string())?;
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
            .ok_or_else(|| format!("{} is not a JSON object", file.destination.path.display()))?;
        Ok(Self { file, json })
    }

    fn account(&self) -> Option<Account> {
        crate::cli::limits::claude_account(&self.json)
    }

    fn with_login(&self, login: &Value) -> Result<Vec<u8>, String> {
        let mut json = self.json.clone();
        account_fields(&mut json, login)?;
        serde_json::to_vec_pretty(&json).map_err(|_| "could not encode the Claude configuration".to_owned())
    }
}

pub(crate) fn account_fields(json: &mut Value, login: &Value) -> Result<(), String> {
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
    Ok(())
}

struct WholeFile {
    destination: Destination,
    bytes: Vec<u8>,
    permissions: Permissions,
}

impl WholeFile {
    fn read(path: PathBuf) -> Result<Self, String> {
        Self::open(Destination::in_login_home(path))
    }

    fn open(destination: Destination) -> Result<Self, String> {
        let read = || -> std::io::Result<_> {
            let mut file = destination.open()?;
            let permissions = file.metadata()?.permissions();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok((bytes, permissions))
        };
        let (bytes, permissions) = read().map_err(|e| format!("could not read {}: {e}", destination.path.display()))?;
        Ok(Self { destination, bytes, permissions })
    }
}
