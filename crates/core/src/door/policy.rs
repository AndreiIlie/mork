//! Resolved operations carry their target's allowlist verdict and exact dispatch envelope.
//! An outside call needs approval; invalid requests never reach dispatch (fac-1, fac-124).

use std::collections::BTreeMap;

use factory_types::door::{
    FILE_GET_MAX, FileCat, FileGet, FileTail, LibraGet, MiscRun, MongoFind, Op, ShRun, SqlSelect,
};
use factory_types::door_call::Verdict;
use factory_types::door_config::{ArgType, Limits, Target, TargetsFile, Transport};
use factory_types::door_plan::{Bounds, Envelope, Plan, Route};
use serde_json::Value;

/// A request resolved: what would run, and the allowlist's verdict on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub envelope: Envelope,
    pub verdict: Verdict,
}

/// The allowlist's verdict on a call it allows.
pub const ALLOWED: &str = "allowed";
const OUTSIDE: &str = "outside";

/// Resolves `op` on `target`, or says why no adapter may run it (`not allowed`).
pub fn resolve(file: &TargetsFile, target: &str, op: &Op) -> Result<Resolved, String> {
    let t =
        file.targets.get(target).ok_or_else(|| format!("no target `{target}`: factory ctl door.help lists them"))?;
    let limits = file.defaults.lowered(t.limits);
    let (plan, verdict, items) = match op {
        Op::SqlSelect(p) => sql(t, target, p, file.defaults)?,
        Op::MongoFind(p) => mongo(t, target, p, limits)?,
        Op::LibraGet(p) => libra(t, target, p)?,
        Op::FileTail(p) => tail(t, target, p, limits)?,
        Op::FileCat(p) => cat(t, target, p)?,
        Op::FileGet(p) => get(t, target, p)?,
        Op::ShRun(p) => sh(t, target, p)?,
        Op::MiscRun(p) => misc(p)?,
    };
    let route = route(t, target, matches!(op, Op::MiscRun(_)))?;
    let timeout_ms =
        if matches!(op, Op::SqlSelect(_)) { super::target::sql_bounds(file.defaults, t).1 } else { limits.timeout_ms };
    // A file's copy is bound by its own cap, never the answer's text.
    let max_bytes = if matches!(op, Op::FileGet(_)) { FILE_GET_MAX } else { limits.max_bytes };
    let bounds = Bounds { max_bytes, max_items: items, timeout_ms };
    Ok(Resolved { envelope: Envelope { target: target.to_owned(), route, plan, bounds }, verdict })
}

// Every call runs by the engine's own dispatcher: on this machine, or over the target's alias,
// allowed only the kinds the target configures; the misc door's, only its argv.
fn route(t: &Target, target: &str, misc: bool) -> Result<Route, String> {
    if misc && !t.misc {
        return Err(format!("`{target}` has no misc door"));
    }
    match (t.transport, &t.ssh) {
        (Transport::Local, _) => Ok(Route::Local { allow: kinds(t) }),
        (Transport::Ssh, Some(s)) => {
            let allow = if misc { vec!["sh".to_owned()] } else { kinds(t) };
            Ok(Route::Ssh { alias: s.alias.clone(), allow, misc })
        }
        (Transport::Ssh, None) => Err(format!("`{target}` is reached by ssh, but names no alias")),
    }
}

fn kinds(t: &Target) -> Vec<String> {
    let has = [
        ("sql", t.sql.is_some()),
        ("mongo", t.mongo.is_some()),
        ("libra", t.libra.is_some()),
        ("file", t.file.is_some()),
        ("sh", !t.sh.is_empty()),
    ];
    has.iter().filter(|(_, on)| *on).map(|(k, _)| (*k).to_owned()).collect()
}

type Step = (Plan, Verdict, u64);

fn schema_refused(hint: Option<&str>) -> String {
    match hint.map(str::trim).filter(|h| !h.is_empty()) {
        Some(hint) => format!("{} {hint}", super::sql::SCHEMA_REFUSED),
        None => super::sql::SCHEMA_REFUSED.to_owned(),
    }
}

// Any read, on any table (fac-124): the login's own grants and the read-only session it runs in
// bound it, the gate only names a write first.
fn sql(t: &Target, target: &str, p: &SqlSelect, defaults: Limits) -> Result<Step, String> {
    let s = t.sql.as_ref().ok_or_else(|| format!("`{target}` has no SQL"))?;
    let (cap, timeout_ms) = super::target::sql_bounds(defaults, t);
    // An unqualified read of a target whose database is one of these reads that schema.
    if ["information_schema", "sys"].iter().any(|d| s.database.eq_ignore_ascii_case(d)) {
        return Err(schema_refused(s.schema_hint.as_deref()));
    }
    let read = super::sql::read(&p.query)
        .map_err(|e| if e == super::sql::SCHEMA_REFUSED { schema_refused(s.schema_hint.as_deref()) } else { e })?;
    let plan = Plan::Sql {
        login: s.login.clone(),
        host: s.host.clone(),
        port: s.port,
        database: s.database.clone(),
        query: read.capped(cap.unwrap_or(u64::MAX))?,
    };
    read.timed(timeout_ms)?;
    Ok((plan, Verdict::new(ALLOWED, "a read"), cap.unwrap_or(u64::MAX)))
}

fn mongo(t: &Target, target: &str, p: &MongoFind, limits: Limits) -> Result<Step, String> {
    let m = t.mongo.as_ref().ok_or_else(|| format!("`{target}` has no Mongo"))?;
    super::mongo::collection(&p.collection)?;
    super::mongo::filter(&p.filter)?;
    super::mongo::sort(&p.sort)?;
    let fields = super::mongo::projection(&p.projection)?;
    let max = limits.max_documents;
    // Mongo reads a zero limit as none at all.
    let limit = match p.limit {
        Some(0) => return Err(format!("`limit` is 1 to {max}")),
        Some(n) if n <= max => n,
        _ => max.saturating_add(1),
    };
    let plan = Plan::Mongo {
        login: m.login.clone(),
        host: m.host.clone(),
        port: m.port,
        database: m.database.clone(),
        auth_database: m.auth_database.clone(),
        collection: p.collection.clone(),
        query: Value::Object(p.filter.clone()),
        fields,
        sort: Value::Object(p.sort.clone()),
        limit,
    };
    Ok((plan, Verdict::new(ALLOWED, "a find"), max))
}

// Endpoint names are templates, not permissions (fac-124); all GETs stay at the configured origin.
fn libra(t: &Target, target: &str, p: &LibraGet) -> Result<Step, String> {
    let l = t.libra.as_ref().ok_or_else(|| format!("`{target}` has no Libra"))?;
    let template = l.endpoints.get(&p.endpoint);
    let mut path = match template {
        Some(e) => e.path.clone(),
        None if get_path(&p.endpoint) => p.endpoint.clone(),
        None => return Err(format!("no alias `{}`: use an endpoint alias or a GET path `/…`", p.endpoint)),
    };
    let mut query = Vec::new();
    for (k, v) in &p.params {
        let value = encoded(v);
        let slot = format!("{{{k}}}");
        if template.is_some() && path.contains(&slot) {
            path = path.replace(&slot, &value);
        } else {
            query.push(format!("{}={value}", encoded(k)));
        }
    }
    if template.is_some()
        && let Some(missing) = placeholders(&path).unwrap_or_default().first()
    {
        return Err(format!("`{}` needs its parameter `{missing}`", p.endpoint));
    }
    path = encoded_path(&path);
    if !query.is_empty() {
        let separator = if path.contains('?') { '&' } else { '?' };
        path = format!("{path}{separator}{}", query.join("&"));
    }
    let plan = Plan::Libra {
        client: l.client.clone(),
        origin: l.origin.clone(),
        token_path: l.token_path.clone(),
        scope: l.scope.clone(),
        path,
    };
    Ok((plan, Verdict::new(ALLOWED, "a GET at the configured origin"), 0))
}

/// An origin-relative GET path; URL syntax is encoded before dispatch.
pub(super) fn get_path(p: &str) -> bool {
    p.starts_with('/') && !p.chars().any(char::is_control)
}

fn encoded_path(path: &str) -> String {
    path.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => char::from(b).to_string(),
            _ if b"/_.-~!$&'()*+,;=:@%?".contains(&b) => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The `{name}`s of a path, or `None` when its braces don't pair.
pub(super) fn placeholders(path: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(i) = rest.find(['{', '}']) {
        let after = &rest[i + 1..];
        let end = after.find('}').filter(|_| rest.as_bytes()[i] == b'{')?;
        let name = &after[..end];
        if name.is_empty() || name.contains('{') {
            return None;
        }
        names.push(name.to_owned());
        rest = &after[end + 1..];
    }
    Some(names)
}

// Percent-encoded: everything but RFC 3986's unreserved characters.
fn encoded(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn tail(t: &Target, target: &str, p: &FileTail, limits: Limits) -> Result<Step, String> {
    let path =
        t.file.as_ref().and_then(|f| f.logs.get(&p.log)).ok_or_else(|| format!("no log `{}` on `{target}`", p.log))?;
    let lines = p.lines.unwrap_or(limits.max_lines).min(limits.max_lines);
    Ok((Plan::Tail { path: path.clone(), lines }, Verdict::new(ALLOWED, "a named log"), limits.max_lines))
}

fn cat(t: &Target, target: &str, p: &FileCat) -> Result<Step, String> {
    let root = t
        .file
        .as_ref()
        .and_then(|f| f.roots.get(&p.root))
        .ok_or_else(|| format!("no root `{}` on `{target}`", p.root))?;
    relative(&p.path)?;
    Ok((Plan::Read { root: root.clone(), path: p.path.clone() }, Verdict::new(ALLOWED, "a file under a named root"), 0))
}

fn get(t: &Target, target: &str, p: &FileGet) -> Result<Step, String> {
    let root = t
        .file
        .as_ref()
        .and_then(|f| f.roots.get(&p.root))
        .ok_or_else(|| format!("no root `{}` on `{target}`", p.root))?;
    relative(&p.path)?;
    Ok((Plan::Get { root: root.clone(), path: p.path.clone() }, Verdict::new(ALLOWED, "a file under a named root"), 0))
}

fn relative(path: &str) -> Result<(), String> {
    let ok = !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    if ok { Ok(()) } else { Err(format!("`{path}` isn't a path under its root: relative, no `.` or `..`")) }
}

fn sh(t: &Target, target: &str, p: &ShRun) -> Result<Step, String> {
    let c =
        t.sh.get(&p.command)
            .ok_or_else(|| format!("no command `{}` on `{target}`: misc.run asks Andrei for any other", p.command))?;
    if let Some(k) = p.args.keys().find(|k| !c.args.contains_key(*k)) {
        return Err(format!("`{}` takes no argument `{k}`", p.command));
    }
    let mut given = BTreeMap::new();
    for (name, ty) in &c.args {
        let v = p.args.get(name).ok_or_else(|| format!("`{}` needs its argument `{name}`", p.command))?;
        given.insert(format!("{{{name}}}"), arg(*ty, v).map_err(|e| format!("`{name}`: {e}"))?);
    }
    let argv = c.argv.iter().map(|a| given.get(a).cloned().unwrap_or_else(|| a.clone())).collect();
    Ok((Plan::Run { argv }, Verdict::new(ALLOWED, "a named command of the target"), 0))
}

fn misc(p: &MiscRun) -> Result<Step, String> {
    if p.argv.is_empty() || p.argv.len() > 64 || p.argv[0].is_empty() {
        return Err("misc.run takes an argv of 1 to 64 arguments".into());
    }
    if p.argv.iter().any(|a| a.len() > 4096 || a.contains('\0')) {
        return Err("an argument is over 4096 bytes, or holds a NUL".into());
    }
    let verdict = Verdict::new(OUTSIDE, "the misc door always asks Andrei");
    Ok((Plan::Run { argv: p.argv.clone() }, verdict, 0))
}

fn word(w: &str) -> Result<(), String> {
    let ok = !w.is_empty()
        && w.len() <= 128
        && !w.starts_with('-')
        && w.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'@' | b':' | b'-'));
    if ok { Ok(()) } else { Err(format!("`{w}` isn't a word: letters, digits and `_.@:-`, not first `-`")) }
}

// A typed argument as the one literal argument it becomes. None begins with `-`, so none is
// read as an option.
fn arg(ty: ArgType, v: &Value) -> Result<String, String> {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) if ty == ArgType::U64 && n.is_u64() => n.to_string(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            return Err(format!("a {} is a string", ty.word()));
        }
    };
    let printable = !text.chars().any(char::is_control);
    let ok = match ty {
        ArgType::Word => word(&text).is_ok(),
        ArgType::U64 => !text.is_empty() && text.len() <= 20 && text.bytes().all(|b| b.is_ascii_digit()),
        ArgType::Date => date(&text),
        ArgType::Path => !text.is_empty() && text.len() <= 4096 && printable && !text.starts_with('-'),
        ArgType::Text => !text.is_empty() && text.len() <= 1024 && printable && !text.starts_with('-'),
    };
    if ok { Ok(text) } else { Err(format!("`{text}` isn't a {}", ty.word())) }
}

fn date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}
