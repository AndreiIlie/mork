//! Target policies are validated whole and described without connection details or credentials.
//! Descriptions expose the operations and bounds an agent may request (fac-1, fac-124).

use factory_types::door_config::{Libra, Limits, Login, Sql, Target, TargetsFile, Transport};
use serde_json::{Value, json};

// The longest a read-only server's SQL may be given: an hour.
const MAX_SQL_MS: u64 = 3_600_000;

/// What is wrong with a targets file, if anything: checked whole when it is read, so a call
/// never runs on half a configuration.
pub fn check_file(f: &TargetsFile) -> Result<(), String> {
    if f.version == 1 {
        return Err("version 1 is the dedicated dispatcher's format, which is gone: rewrite it for ssh aliases \
                    and access sections (version 2)"
            .into());
    }
    if f.version != factory_types::door_config::TARGETS_VERSION {
        return Err(format!("version {} isn't one this build reads (2)", f.version));
    }
    if f.access.as_ref().is_some_and(|a| !real(a)) {
        return Err("access is the access file's absolute path".into());
    }
    for (id, t) in &f.targets {
        check_target(t, f.access.is_some()).map_err(|e| format!("[targets.{id}]: {e}"))?;
    }
    Ok(())
}

fn real(p: &str) -> bool {
    p.starts_with('/') && p.split('/').all(|c| c != "." && c != "..")
}

// A name passed as one argument or matched as written: an alias, a section, a field, a host.
fn name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

fn check_target(t: &Target, access: bool) -> Result<(), String> {
    match (t.transport, &t.ssh) {
        (Transport::Ssh, None) => return Err("transport = \"ssh\" needs [ssh]".into()),
        (Transport::Local, Some(_)) => return Err("transport = \"local\" takes no [ssh]".into()),
        (Transport::Ssh, Some(s)) if !name(&s.alias) => return Err("[ssh] needs an alias of Andrei's".into()),
        (Transport::Ssh | Transport::Local, _) => {}
    }
    if t.misc && t.transport != Transport::Ssh {
        return Err("the misc door runs over a target's ssh alias: transport = \"ssh\"".into());
    }
    check_services(t, access)?;
    // The dispatcher opens them from `/`, following no symlink: each is a real, absolute path.
    if let Some(f) = &t.file
        && let Some(bad) = f.roots.values().chain(f.logs.values()).find(|p| !real(p))
    {
        return Err(format!("[file] roots and logs are absolute paths, with no symlink on them: `{bad}`"));
    }
    if let Some(s) = &t.sql
        && s.timeout_ms.is_some_and(|ms| !s.read_only || ms == 0 || ms > MAX_SQL_MS)
    {
        return Err(format!("[sql] timeout_ms is a read-only server's, from 1 to {MAX_SQL_MS}"));
    }
    for (name, c) in &t.sh {
        check_command(name, c)?;
    }
    Ok(())
}

// Each service names its credential in the access file, and its server as the target sees it.
fn check_services(t: &Target, access: bool) -> Result<(), String> {
    let login = |l: &Login| name(&l.section) && name(&l.user) && name(&l.password);
    let server = |h: &str, p: u16| name(h) && p != 0;
    if let Some(s) = &t.sql
        && !(login(&s.login) && server(&s.host, s.port))
    {
        return Err("[sql] needs a login (an access section and its fields) and a host and port".into());
    }
    if let Some(m) = &t.mongo
        && !(login(&m.login) && server(&m.host, m.port) && m.auth_database.as_deref().is_none_or(name))
    {
        return Err("[mongo] needs a login (an access section and its fields) and a host and port".into());
    }
    if let Some(l) = &t.libra {
        check_libra(l)?;
    }
    if !access && (t.sql.is_some() || t.mongo.is_some() || t.libra.is_some()) {
        return Err("its credentials are sections of the access file: name it, as `access`".into());
    }
    Ok(())
}

fn check_libra(l: &Libra) -> Result<(), String> {
    let c = &l.client;
    if !(name(&c.section) && name(&c.key) && name(&c.secret)) {
        return Err("[libra] needs a client (an access section and its fields)".into());
    }
    let host = l.origin.strip_prefix("https://").unwrap_or("");
    if !name(host) {
        return Err("[libra] origin is `https://<host>:<port>`, nothing more".into());
    }
    if !super::policy::get_path(&l.token_path) || !name(&l.scope) {
        return Err("[libra] needs a token_path (`/…`) and a scope".into());
    }
    for (id, e) in &l.endpoints {
        let names = super::policy::placeholders(&e.path);
        if names.is_none() || !super::policy::get_path(&e.path.replace(['{', '}'], "")) {
            return Err(format!("[libra.endpoints.{id}]: its path is `/…`, paired `{{name}}` placeholders"));
        }
    }
    Ok(())
}

// The program is fixed; each `{arg}` is a whole, declared element, and each declared one used.
fn check_command(name: &str, c: &factory_types::door_config::ShCommand) -> Result<(), String> {
    let Some(first) = c.argv.first() else { return Err(format!("[sh.{name}] has an empty argv")) };
    if first.contains('{') {
        return Err(format!("[sh.{name}]'s program is fixed: no `{{arg}}` in it"));
    }
    for a in &c.argv {
        let whole = a.starts_with('{') && a.ends_with('}') && a.len() > 2;
        if a.contains(['{', '}']) && !(whole && c.args.contains_key(&a[1..a.len() - 1])) {
            return Err(format!("[sh.{name}]: `{a}` isn't a whole, declared argument"));
        }
    }
    if let Some(unused) = c.args.keys().find(|k| !c.argv.contains(&format!("{{{k}}}"))) {
        return Err(format!("[sh.{name}] declares `{unused}` but its argv doesn't use it"));
    }
    Ok(())
}

/// A target as an agent sees it: what it is, what it offers and its bounds;
/// no alias, host, login or credential.
pub fn describe(f: &TargetsFile, id: &str, t: &Target) -> Value {
    let limits = f.defaults.lowered(t.limits);
    let mut v = json!({
        "target": id,
        "description": t.description,
        "transport": match t.transport { Transport::Local => "local", Transport::Ssh => "ssh" },
        "limits": limits,
        "misc": t.misc,
    });
    if let Some(s) = &t.sql {
        v["sql"] = sql(f, t, s);
    }
    if let Some(m) = &t.mongo {
        v["mongo"] =
            json!({"database": m.database, "collections": "every collection its login may read", "read_only": true});
    }
    if let Some(l) = &t.libra {
        v["libra"] =
            json!({"endpoints": l.endpoints, "paths": "any GET path at the configured origin", "read_only": true});
    }
    if let Some(fl) = &t.file {
        v["file"] = json!({"roots": fl.roots, "logs": fl.logs});
    }
    if !t.sh.is_empty() {
        v["sh"] = json!(t.sh);
    }
    v
}

// What any target's SQL reads (fac-124): every table its login may read, in a read-only session.
const READS: &[&str] = &[
    "one statement beginning SELECT, WITH, TABLE, VALUES, SHOW, DESCRIBE, DESC, EXPLAIN or `(`",
    "never INSERT, UPDATE, DELETE, REPLACE or INTO, even under EXPLAIN: it runs in a read-only session",
    "locking reads only under EXPLAIN without ANALYZE, which only plans",
];

// A read-only server's bounds are its SQL's own: `max_rows` null caps no rows.
fn sql(f: &TargetsFile, t: &Target, s: &Sql) -> Value {
    let (max_rows, timeout_ms) = sql_bounds(f.defaults, t);
    json!({
        "database": s.database,
        "tables": "every table its login may read, except information_schema and the sys schema",
        "read_only": true,
        "read_only_server": s.read_only,
        "reads": READS,
        "max_rows": max_rows,
        "timeout_ms": timeout_ms,
    })
}

/// A target's SQL row cap (none: the query's own LIMIT alone) and timeout. A read-only server
/// keeps only the target's own cap, and may wait past the default.
pub(super) fn sql_bounds(defaults: Limits, t: &Target) -> (Option<u64>, u64) {
    let l = defaults.lowered(t.limits);
    match &t.sql {
        Some(s) if s.read_only => (t.limits.max_rows, s.timeout_ms.unwrap_or(l.timeout_ms)),
        _ => (Some(l.max_rows), l.timeout_ms),
    }
}

pub fn describe_all(f: &TargetsFile) -> Value {
    Value::Array(f.targets.iter().map(|(id, t)| describe(f, id, t)).collect())
}
