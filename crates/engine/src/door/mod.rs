//! The door's targets (fac-1): `targets.toml` is re-read at each request and before each dispatch;
//! the credentials it names live in the operator's access file, which only the engine reads.

pub mod access;
pub mod call;
pub mod output;
pub mod redact;

use std::path::Path;

use mork_core::door::policy::Resolved;
use mork_types::door::{Op, canonical};
use mork_types::door_config::TargetsFile;
use mork_types::door_plan::{Pair, Plan};

use crate::paths::Home;
use redact::Redactor;

/// Reads and checks `targets.toml`; an error names its line where it has one, and never the
/// home's path: its words reach agents.
pub fn read(home: &Home) -> Result<TargetsFile, String> {
    let text = std::fs::read_to_string(home.targets()).map_err(|e| format!("targets.toml: {e}"))?;
    parse(&text).map_err(|e| format!("targets.toml: {e}"))
}

/// Parses targets strictly.
pub fn parse(text: &str) -> Result<TargetsFile, String> {
    let f: TargetsFile = toml::from_str(text).map_err(|e: toml::de::Error| {
        let line = e.span().map(|s| text.get(..s.start).map_or(1, |t| t.matches('\n').count() + 1));
        match line {
            Some(l) => format!("line {l}: {}", e.message().trim()),
            None => e.message().trim().to_owned(),
        }
    })?;
    mork_core::door::target::check_file(&f)?;
    Ok(f)
}

/// A request resolved now, with its envelope's canonical text: what an approval binds to.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub resolved: Resolved,
    pub plan: String,
}

/// Resolves `op` on `target` against the file as it is now.
pub fn resolve(f: &TargetsFile, target: &str, op: &Op) -> Result<Resolution, String> {
    let resolved = mork_core::door::policy::resolve(f, target, op)?;
    let plan = canonical(&serde_json::to_value(&resolved.envelope).map_err(|e| e.to_string())?);
    Ok(Resolution { resolved, plan })
}

/// A call resolved again from `targets.toml` as it is now: its plan and its allowlist verdict.
pub fn current(home: &Home, c: &mork_types::door_call::DoorCall) -> Result<Resolution, String> {
    let f = read(home)?;
    let params: serde_json::Value = serde_json::from_str(&c.params).map_err(|e| e.to_string())?;
    let op = Op::parse(&c.op, &params).map_err(|e| format!("{e:?}"))?;
    resolve(&f, &c.target, &op)
}

/// Whether a call still resolves to the plan and the allowlist verdict it was cleared or
/// approved on: a changed configuration or a revoked allowlist stops it.
pub fn unchanged(home: &Home, a: &mork_types::action::Action) -> Result<(), String> {
    let (c, d) = mork_core::door::parts(a).ok_or("not a door call")?;
    let now = current(home, c).map_err(|e| format!("its target can't be resolved now: {e}"))?;
    if now.plan != c.plan || now.resolved.verdict != d.validator {
        return Err("the target's configuration or allowlist changed since it was checked: ask again".into());
    }
    Ok(())
}

/// A plan's credential and every secret value of the access file, for the dispatcher to mask.
/// A file configured but unreadable is an error: its secrets couldn't be masked.
pub fn credentials(home: &Home, plan: &Plan) -> Result<(Option<Pair>, Vec<String>), String> {
    let f = read(home)?;
    let Some(path) = &f.access else {
        return match plan {
            Plan::Sql { .. } | Plan::Mongo { .. } | Plan::Http { .. } => {
                Err("targets.toml names no access file for its credentials".into())
            }
            _ => Ok((None, Vec::new())),
        };
    };
    let file = access::read(Path::new(path))?;
    let pair = match plan {
        Plan::Sql { login, .. } => Some(file.pair(&login.section, &login.user, &login.password, true)?),
        Plan::Mongo { login, .. } => Some(file.pair(&login.section, &login.user, &login.password, false)?),
        Plan::Http { client, .. } => Some(file.pair(&client.section, &client.key, &client.secret, false)?),
        _ => None,
    };
    let mut secrets = file.secrets();
    if let Some(p) = &pair {
        secrets.push(p.secret.clone());
        if matches!(plan, Plan::Http { .. }) {
            secrets.push(p.name.clone());
        }
    }
    Ok((pair, secrets))
}

/// What door output is masked of on this side: every secret the leak check knows, and the access
/// file's own, whatever their length.
pub fn redactor(home: &Home, known: &crate::secrets::Secrets) -> Redactor {
    let mut r = Redactor::default();
    let access = read(home).ok().and_then(|f| f.access).and_then(|p| access::read(Path::new(&p)).ok());
    for v in access.iter().flat_map(access::AccessFile::secrets) {
        r.add(&v);
    }
    for v in known.values() {
        r.add(v);
    }
    r
}
