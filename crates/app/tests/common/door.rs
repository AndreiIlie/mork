//! Door fixtures keep credentials outside walls across local, replica and SSH paths.
//! Sentinel secrets make disclosure observable across adapters (fac-1, fac-86, fac-92).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::Rig;

/// The local target's read-only MySQL password, in `[access:host-db]`.
pub const HOST_PW: &str = "SENTINEL-host-db-77";
/// The local target's Mongo password, in `[access:host-mongo]`.
pub const MONGO_PW: &str = "SENTINEL-host-mongo-55";
/// The remote targets' MySQL password, in `[access:app-db]`.
pub const REMOTE_PW: &str = "SENTINEL-app-db-31";
/// The remote targets' Mongo password, in `[access:app-mongo]`.
pub const REMOTE_MONGO_PW: &str = "SENTINEL-app-mongo-42";
/// The OAuth client key, in `[access:example-api]`.
pub const HTTP_KEY: &str = "SENTINEL-http-consumer-key-99";
/// The OAuth client secret, in `[access:example-api]`.
pub const HTTP_SECRET: &str = "SENTINEL-http-consumer-secret-88";
/// The bearer token the API hands back for the key and secret.
pub const HTTP_TOKEN: &str = "SENTINEL-http-bearer-token-66";
/// The refresh token it hands back beside it, never used.
pub const HTTP_REFRESH: &str = "SENTINEL-http-refresh-token-44";
/// A sudo password elsewhere in the access file: held, never a permission (fac-86).
pub const SUDO_PW: &str = "SENTINEL-sudo-pw-13";

/// Every sentinel: a leak test looks for each everywhere afterwards.
pub const SENTINELS: [&str; 9] =
    [HOST_PW, MONGO_PW, REMOTE_PW, REMOTE_MONGO_PW, HTTP_KEY, HTTP_SECRET, HTTP_TOKEN, HTTP_REFRESH, SUDO_PW];

/// The operator's own ssh aliases, each its box's host, login and keys; distinct from the target id.
pub const STAGING_ALIAS: &str = "ex-staging-box";
pub const PRODUCTION_ALIAS: &str = "ex-prod-box";

impl Rig {
    /// The targets' own files: `remote/` holds what lives on the remote targets.
    pub fn remote(&self) -> PathBuf {
        self.t.root.join("remote")
    }

    /// The operator's synthetic access file, outside the walls, readable by its owner alone.
    pub fn access_file(&self) -> PathBuf {
        self.t.root.join("access.md")
    }

    /// Writes the door's targets, the access file and the fake world; the app reads them at each
    /// request.
    pub fn door(&mut self) {
        let remote = self.remote();
        std::fs::create_dir_all(remote.join("srv/app/config")).unwrap();
        std::fs::write(remote.join("srv/app/config/app.yml"), "name: app\nmode: staging\n").unwrap();
        std::fs::write(remote.join("app.log"), (1..=300).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
        std::fs::write(remote.join("creds.log"), format!("connected with {HOST_PW}\n")).unwrap();
        let _ = std::os::unix::fs::symlink("/etc/passwd", remote.join("srv/app/passwd"));
        self.write_access();
        std::fs::write(self.fhome().join("targets.toml"), targets(&self.access_file(), &remote)).unwrap();
        self.door_world();
    }

    // Synthetic credentials, and sudo decoys the door never follows or runs.
    fn write_access(&self) {
        use std::os::unix::fs::PermissionsExt;
        let never = self.t.root.join("never-read");
        let tunnel_ran = self.t.root.join("tunnel-ran");
        let text = format!(
            "# The operator's access (synthetic, for tests)\n\n\
             [access:host-db]\nuser = door_ro\npassword = {HOST_PW}\n\n\
             [access:host-mongo]\nuser = door_ro\npassword = {MONGO_PW}\n\n\
             [access:app-db]\nuser = door_ro\npassword = {REMOTE_PW}\n\n\
             [access:app-mongo]\nuser = door_ro\npassword = {REMOTE_MONGO_PW}\n\n\
             [access:example-api]\nconsumer_key = {HTTP_KEY}\nconsumer_secret = {HTTP_SECRET}\n\n\
             [access:sudo]\nuser = operator\npassword = {SUDO_PW}\n\
             credentials_file = {never}\ntunnel_command = touch {tunnel_ran}\n",
            never = never.display(),
            tunnel_ran = tunnel_ran.display(),
        );
        let path = self.access_file();
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn door_world(&mut self) {
        let world = json!({
            "ssh": { STAGING_ALIAS: {}, PRODUCTION_ALIAS: {} },
        });
        self.door_fake(world);
    }

    /// Writes the fakes' door scenario, and names it in the app's environment.
    pub fn door_fake(&mut self, world: Value) {
        let path = self.t.root.join("door-world.json");
        std::fs::write(&path, world.to_string()).unwrap();
        self.set_env("FAKE_DOOR", &path.display().to_string());
    }

    /// The fakes' door scenario as it is.
    pub fn door_world_now(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.t.root.join("door-world.json")).unwrap()).unwrap()
    }

    /// A door fake's runs, with their literal argv and stdin.
    pub fn door_runs(&self, name: &str) -> Vec<Value> {
        mork_fakes::door::runs(&self.log_path(), name)
    }
}

fn targets(access: &std::path::Path, remote: &std::path::Path) -> String {
    let r = remote.display();
    let a = access.display();
    format!(
        "version = 2\naccess = \"{a}\"\n\
         [targets.host]\ndescription = \"this machine\"\ntransport = \"local\"\n\
         [targets.host.sql]\ndatabase = \"app\"\nlogin = {{ section = \"host-db\" }}\n\
         [targets.host.mongo]\ndatabase = \"logs\"\nlogin = {{ section = \"host-mongo\" }}\n\
         [targets.host.file]\nroots = {{ app = \"{r}/srv/app\" }}\nlogs = {{ app = \"{r}/app.log\", creds = \"{r}/creds.log\" }}\n\
         [targets.host.sh.echo]\nabout = \"prints its argument\"\nargv = [\"printf\", \"%s\", \"{{path}}\"]\nargs = {{ path = \"path\" }}\n\
         [targets.replica]\ndescription = \"a read-only copy\"\ntransport = \"local\"\n\
         [targets.replica.sql]\ndatabase = \"app\"\nread_only = true\ntimeout_ms = 600000\nlogin = {{ section = \"host-db\" }}\n\
         {}{}",
        ssh_target("staging", STAGING_ALIAS, &r),
        ssh_target("production", PRODUCTION_ALIAS, &r),
    )
}

// `ssh` and `misc` are bare keys, so they come before the service tables.
fn ssh_target(id: &str, alias: &str, r: &std::path::Display) -> String {
    format!(
        "[targets.{id}]\ndescription = \"{id}: the app's servers\"\ntransport = \"ssh\"\n\
         ssh = {{ alias = \"{alias}\" }}\nmisc = true\n\
         [targets.{id}.sql]\ndatabase = \"app\"\nlogin = {{ section = \"app-db\" }}\n\
         [targets.{id}.mongo]\ndatabase = \"logs\"\nlogin = {{ section = \"app-mongo\" }}\n\
         [targets.{id}.http]\norigin = \"https://api.example.invalid:8243\"\n\
         client = {{ section = \"example-api\", key = \"consumer_key\", secret = \"consumer_secret\" }}\n\
         [targets.{id}.http.endpoints.invoices]\nabout = \"invoices by date\"\npath = \"/v1/invoices\"\n\
         [targets.{id}.file]\nroots = {{ app = \"{r}/srv/app\" }}\nlogs = {{ app = \"{r}/app.log\" }}\n\
         [targets.{id}.sh.echo]\nabout = \"prints its argument\"\nargv = [\"printf\", \"%s\", \"{{path}}\"]\nargs = {{ path = \"path\" }}\n"
    )
}

/// `mork ctl door …`'s arguments.
pub fn door_args(target: &str, op: &str, params: Value) -> Vec<String> {
    ["door", "--target", target, "--op", op, "--params", &params.to_string()].iter().map(|s| (*s).to_owned()).collect()
}

/// Counts ssh runs whose last argv element, the remote string, holds `'<word>'` as a single-quoted argument;
/// a forward or file read never does (a read's words are a path's, so pick a word no path holds).
pub fn remote_commands(rig: &Rig, word: &str) -> usize {
    let needle = format!("'{word}'");
    rig.door_runs("ssh")
        .iter()
        .filter(|r| {
            r["argv"]
                .as_array()
                .and_then(|v| v.last())
                .and_then(Value::as_str)
                .is_some_and(|last| last.contains(&needle))
        })
        .count()
}

/// A fake Codex's turn of `agent` that runs `mork ctl` with each of `calls`, then replies.
pub fn ctl_turn(agent: &str, phrase: Option<&str>, calls: &[Vec<String>], reply: Value) -> Value {
    let mut steps = vec![json!({"do": "thread-started"})];
    steps.extend(calls.iter().map(|c| json!({"do": "ctl", "args": c})));
    steps.push(json!({"do": "codex-reply", "value": reply}));
    json!({"fake": "codex", "agent": agent, "phrase": phrase, "steps": steps})
}

/// What each `mork ctl` of a job answered, in order: its JSON, or its error.
pub fn answers(rig: &Rig, job: &mork_types::state::Job) -> Vec<Value> {
    rig.out_lines(job)
        .into_iter()
        .filter(|v| v["type"] == "fake-ctl")
        .map(|v| {
            let out = v["out"].as_str().unwrap_or("").trim().to_owned();
            serde_json::from_str::<Value>(&out)
                .unwrap_or_else(|_| json!({"raw": out, "err": v["err"], "code": v["code"]}))
        })
        .collect()
}

/// Every text an agent, a wall or a reader could see, and what is kept (job folders, records, logs, receipts,
/// calls' folders, store ids, every state row), JSON escapes decoded at every depth.
pub fn everything(rig: &Rig) -> String {
    let mut out = String::new();
    let dirs: Vec<PathBuf> = [
        "jobs",
        "records",
        "logs",
        "fixtures",
        "agent-home",
        "homes",
        "work",
        "caches",
        "logins",
        "door/receipts",
        "door/calls",
        "door/tunnels",
        "door/stores",
    ]
    .iter()
    .map(|d| rig.fhome().join(d))
    .collect();
    for dir in dirs {
        for f in walk(&dir) {
            out.push_str(&String::from_utf8_lossy(&std::fs::read(&f).unwrap_or_default()));
        }
    }
    if let Some(state) = rig.try_state() {
        out.push_str(&format!("{:?}", state));
    }
    for e in rig.store().events().unwrap() {
        out.push_str(&e.3.to_string());
    }
    out.push_str(&decoded_fully(&out));
    out
}

/// A text with JSON's escapes decoded until nothing changes, each depth kept.
pub fn decoded_fully(text: &str) -> String {
    let mut all = String::new();
    let mut t = text.to_owned();
    loop {
        let next = mork_engine::door::redact::unescape_json(&t);
        if next == t {
            return all;
        }
        all.push_str(&next);
        t = next;
    }
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            out.extend(walk(&e.path()));
        } else if t.is_file() {
            out.push(e.path());
        }
    }
    out
}
