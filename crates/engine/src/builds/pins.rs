// A crash at any point of a move is finished by the next start: each is written under an id
// first (`pin.move`, `pin.next`), logged once and its line in main said once (M1.15).

use std::io::Write;

use serde::{Deserialize, Serialize};

use super::BuildError;
use crate::paths::Home;
use crate::runtime::Options;
use crate::{clock, files};

/// `pin.next`: a reload under way (1.8.6.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinNext {
    pub target: String,
    pub id: String,
    pub from: String,
    pub who: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PinMove {
    id: String,
    from: String,
    to: String,
    who: String,
}

/// What a start says in main about a reload, once: the journal keeps `key`. A finished reload
/// names its two builds, `from` and `to`, for what it brought (fac-75).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadNote {
    pub key: String,
    pub text: String,
    pub builds: Option<(String, String)>,
}

pub fn pin(home: &Home) -> Option<String> {
    std::fs::read_to_string(home.pin()).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn logged(home: &Home, id: &str) -> bool {
    std::fs::read_to_string(home.pins_log()).is_ok_and(|t| {
        t.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).any(|v| v["id"] == id)
    })
}

fn log_once(home: &Home, id: &str, from: &str, to: &str, who: &str) -> std::io::Result<()> {
    if logged(home, id) {
        return Ok(());
    }
    let line = serde_json::json!({"time": clock::now_ms(), "from": from, "to": to, "who": who, "id": id});
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(home.pins_log())?;
    writeln!(f, "{line}")?;
    f.sync_all()
}

/// The builds the pin moved to after `time` (ms), in `pins.log`'s order: a `pin` or a reload's.
pub fn moved_to_since(home: &Home, time: i64) -> Vec<String> {
    let text = std::fs::read_to_string(home.pins_log()).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["time"].as_i64().is_some_and(|t| t > time))
        .filter_map(|v| v["to"].as_str().map(str::to_owned))
        .collect()
}

/// Finishes a `factory pin` a crash cut short: logged when the pin moved, else dropped.
pub fn reconcile(home: &Home) -> std::io::Result<()> {
    let Some(m) = files::read_json::<PinMove>(&home.pin_move()).ok().flatten() else { return Ok(()) };
    if pin(home).as_deref() == Some(m.to.as_str()) {
        let link = super::shortcut::installed()?;
        log_once(home, &m.id, &m.from, &m.to, &m.who)?;
        files::link_whole(&link, &super::binary(home, &m.to))?;
    }
    std::fs::remove_file(home.pin_move())
}

/// Moves and logs the pin, updates the terminal shortcut, then clears `pin.move`.
/// A failed log restores the pin; startup finishes an interrupted shortcut update from the record.
pub fn write_pin(home: &Home, to: &str, who: &str) -> std::io::Result<()> {
    reconcile(home)?;
    let link = super::shortcut::installed()?;
    let from = pin(home).unwrap_or_default();
    let id = format!("pin-{}-{}", clock::now_ms(), crate::random::salt());
    let m = PinMove { id: id.clone(), from: from.clone(), to: to.to_owned(), who: who.to_owned() };
    files::write_json(&home.pin_move(), &m)?;
    files::write_whole(&home.pin(), to.as_bytes())?;
    if let Err(e) = log_once(home, &id, &from, to, who) {
        let back = if from.is_empty() {
            std::fs::remove_file(home.pin())
        } else {
            files::write_whole(&home.pin(), from.as_bytes())
        };
        if back.is_ok() {
            let _ = std::fs::remove_file(home.pin_move());
        }
        return Err(e);
    }
    files::link_whole(&link, &super::binary(home, to))?;
    std::fs::remove_file(home.pin_move())
}

/// Before the state is opened: a process started as a reload's target checks that
/// `pin.next` names it and its reload's id, so a stray `--reloaded` migrates nothing (1.8.6.6).
pub fn check_reload_target(opts: &Options) -> Result<(), BuildError> {
    let Some(id) = &opts.reloaded else { return Ok(()) };
    let next = files::read_json::<PinNext>(&opts.home.pin_next())
        .ok()
        .flatten()
        .ok_or_else(|| BuildError::Reload("started as a reload's target, but there is no pin.next".into()))?;
    if next.target != opts.commit {
        return Err(BuildError::Reload(format!(
            "pin.next names build {}, not this one ({})",
            next.target, opts.commit
        )));
    }
    if next.id != *id {
        return Err(BuildError::Reload(format!("pin.next names reload {}, not {id}", next.id)));
    }
    Ok(())
}

/// At start: finishes a reload this process is the target of, or says one failed (1.8.6.6,
/// 1.8.6.8). `pin.next` stays until the line in main is committed (`reload_said`).
pub fn finish_reload(opts: &Options) -> Result<Option<ReloadNote>, BuildError> {
    let home = &opts.home;
    reconcile(home).map_err(|source| BuildError::Io { what: "the pin".into(), source })?;
    let Some(next) = files::read_json::<PinNext>(&home.pin_next()).ok().flatten() else {
        if opts.reloaded.is_some() {
            return Err(BuildError::Reload("started as a reload's target, but there is no pin.next".into()));
        }
        return Ok(None);
    };
    let short = |c: &str| c.chars().take(7).collect::<String>();
    if next.target != opts.commit {
        let text = format!("The reload to {} failed: it didn't finish starting.", short(&next.target));
        return Ok(Some(ReloadNote { key: format!("reload-failed:{}", next.id), text, builds: None }));
    }
    if let Some(id) = &opts.reloaded
        && *id != next.id
    {
        return Err(BuildError::Reload(format!("pin.next names reload {}, not {id}", next.id)));
    }
    crate::faults::hit("reload.after-migrate");
    let pin_io = |source| BuildError::Io { what: "the pin".into(), source };
    let link = super::shortcut::installed().map_err(pin_io)?;
    files::write_whole(&home.pin(), next.target.as_bytes()).map_err(pin_io)?;
    log_once(home, &next.id, &next.from, &next.target, &next.who).map_err(pin_io)?;
    files::link_whole(&link, &super::binary(home, &next.target)).map_err(pin_io)?;
    crate::faults::hit("reload.after-pin");
    let text = format!("Reloaded to {} (from {}).", short(&next.target), short(&next.from));
    let builds = (!next.from.is_empty()).then(|| (next.from.clone(), next.target.clone()));
    Ok(Some(ReloadNote { key: format!("reload:{}", next.id), text, builds }))
}

/// After the reload's line is committed (or found committed): `pin.next` goes.
pub fn reload_said(home: &Home) {
    let _ = std::fs::remove_file(home.pin_next());
}
