//! `mork test`, inside a wall (4.9): a held lease reconnects within 30 s of a reload, or ends the command (exit 3);
//! a lease ask says when it waits for a slot, and is asked again over a new connection if Mork goes away.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use serde_json::{Value, json};

use crate::clock;

const LOST: &str = "the test slot was lost; run it again";
const RECONNECT: Duration = Duration::from_secs(30);

pub(super) struct Link {
    stream: UnixStream,
    lines: Receiver<String>,
    next: u64,
    // A reconnect to another build hands over to its binary.
    build: String,
    home: Option<std::path::PathBuf>,
    // The wait for a slot is said once by this process, over its reconnects.
    told: bool,
}

/// Why `mork test` couldn't talk to Mork.
#[derive(Debug)]
pub enum SlotError {
    NoSocket,
    Connect(std::io::Error),
    Io(std::io::Error),
    Closed,
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSocket => write!(f, "mork test runs inside a wall: no MORK_AGENT_SOCK"),
            Self::Connect(e) => write!(f, "no Mork to ask for a test slot: {e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Closed => write!(f, "Mork closed the connection"),
        }
    }
}

impl std::error::Error for SlotError {}

fn sock() -> Result<String, SlotError> {
    std::env::var("MORK_AGENT_SOCK").ok().filter(|s| !s.is_empty()).ok_or(SlotError::NoSocket)
}

fn connect() -> Result<Link, SlotError> {
    let stream = UnixStream::connect(sock()?).map_err(SlotError::Connect)?;
    let mut reader = BufReader::new(stream.try_clone().map_err(SlotError::Io)?);
    let mut hello = String::new();
    reader.read_line(&mut hello).map_err(SlotError::Io)?;
    let hello: Value = serde_json::from_str(&hello).unwrap_or_default();
    let build = hello["hello"]["build"].as_str().unwrap_or_default().to_owned();
    let home = hello["hello"]["home"].as_str().filter(|h| !h.is_empty()).map(std::path::PathBuf::from);
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        while reader.read_line(&mut buf).is_ok_and(|n| n > 0) {
            if tx.send(std::mem::take(&mut buf)).is_err() {
                break;
            }
        }
    });
    Ok(Link { stream, lines, next: 1, build, home, told: false })
}

impl Link {
    pub(super) fn ask(&mut self, cmd: Value) -> Result<Result<Value, String>, SlotError> {
        Ok(flatten(&self.answer_waiting(&cmd, None)?))
    }

    fn answer(&mut self, cmd: Value) -> Result<Value, SlotError> {
        let id = self.next;
        self.next += 1;
        let line = json!({"id": id, "cmd": cmd}).to_string();
        self.stream.write_all(format!("{line}\n").as_bytes()).map_err(SlotError::Io)?;
        loop {
            let text = self.lines.recv().map_err(|_| SlotError::Closed)?;
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            if v["waiting"]["id"].as_u64() == Some(id) && !self.told {
                self.told = true;
                eprintln!("mork test: waiting for a slot ({})", v["waiting"]["why"].as_str().unwrap_or("none is free"));
            }
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(v);
            }
        }
    }

    // Only reclaiming a held lease has a deadline; startup recovery can outlast it (fac-321).
    fn answer_waiting(&mut self, cmd: &Value, since: Option<std::time::Instant>) -> Result<Value, SlotError> {
        loop {
            let v = self.answer(cmd.clone())?;
            if v["err"]["code"] != "recovering" || since.is_some_and(|at| clock::elapsed(at) >= RECONNECT) {
                return Ok(v);
            }
            clock::sleep(Duration::from_millis(500));
        }
    }

    // Only an answer that the lease is gone ends it.
    fn resume(&mut self, lease: &str, since: std::time::Instant) -> bool {
        self.answer_waiting(&json!({"test.resume": {"lease": lease}}), Some(since)).is_ok_and(|v| v.get("ok").is_some())
    }

    // Lines that arrive meanwhile are not answers.
    fn open(&self) -> bool {
        loop {
            match self.lines.try_recv() {
                Ok(_) => {}
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }
}

fn flatten(v: &Value) -> Result<Value, String> {
    match (v.get("ok"), v.get("err")) {
        (Some(ok), _) => Ok(ok.clone()),
        (None, Some(e)) => Err(e.get("message").and_then(Value::as_str).unwrap_or("refused").to_owned()),
        (None, None) => Err("an answer with neither ok nor err".into()),
    }
}

/// Asks for a lease of `kind`, which may wait for a slot as long as it takes: Mork going away meanwhile (a
/// reload, a restart, a new build) is waited for, and the ask made again to it. The `Err` is the exit code.
pub(super) fn lease(link: &mut Link, args: &[String], own: &str, kind: &str, command: &[String]) -> Result<Value, i32> {
    let cmd = json!({"test.lease": {"kind": kind, "command": command}});
    if kind == "walls" {
        return match link.answer_waiting(&cmd, None) {
            Ok(v) => flatten(&v).map_err(|e| fail(&e, 2)),
            Err(_) => Err(fail(LOST, 3)),
        };
    }
    loop {
        match link.answer_waiting(&cmd, None) {
            Ok(v) if v["err"]["code"] == "reloading" => clock::sleep(Duration::from_millis(500)),
            Ok(v) => return flatten(&v).map_err(|e| fail(&e, 2)),
            Err(SlotError::Closed | SlotError::Io(_)) => *link = back(link.told, args, own)?,
            Err(e) => return Err(fail(&e, 2)),
        }
    }
}

// A new connection, however long Mork takes; another build is handed the whole command.
fn back(told: bool, args: &[String], own: &str) -> Result<Link, i32> {
    loop {
        clock::sleep(Duration::from_millis(300));
        if let Ok(mut l) = connect() {
            if !l.build.is_empty() && l.build != own {
                return Err(other_build(&l, args));
            }
            l.told = told;
            return Ok(l);
        }
    }
}

// The turn's `MORK_IO`, its seat's folder (fac-654); a job given none, its own `io/`.
pub(super) fn io_dir() -> String {
    if let Some(io) = std::env::var("MORK_IO").ok().filter(|io| !io.is_empty()) {
        return io;
    }
    let home = sock()
        .ok()
        .and_then(|s| std::path::Path::new(&s).parent().and_then(|r| r.parent()).map(std::path::Path::to_path_buf));
    let job = std::env::var("MORK_JOB").unwrap_or_default();
    home.map(|h| h.join("jobs").join(job).join("io").display().to_string()).unwrap_or_default()
}

fn parse(args: &[String]) -> (&'static str, Vec<String>) {
    match args.first().map(String::as_str) {
        Some("--checks") => ("checks", args[1..].to_vec()),
        Some("--run") => ("run", args[1..].to_vec()),
        Some("--file") => ("file", args[1..].to_vec()),
        Some("--walls") => ("walls", args[1..].to_vec()),
        _ => ("suite", args.to_vec()),
    }
}

/// `mork test …`; its exit code. `own` is this binary's commit.
pub fn run(args: &[String], own: &str) -> i32 {
    if args.first().map(String::as_str) == Some("--resume") {
        return resume_cmd(args, own);
    }
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{}", mork_core::maintenance::manuals::test_help());
        return 0;
    }
    let (kind, command) = parse(args);
    let mut link = match connect() {
        Ok(l) => l,
        Err(e) => return fail(&e, 2),
    };
    // Mork of another build: its binary runs instead, with the same arguments (1.6.4).
    if !link.build.is_empty() && link.build != own {
        return other_build(&link, args);
    }
    // The engine's checks stage, on the clone as it stands (fac-165, fac-261).
    if kind == "checks" {
        return super::local_checks::run(link, args, &command, own);
    }
    // A suite is refused, with arguments or none: the merge train runs it (fac-10, fac-65).
    let answer = match lease(&mut link, args, own, kind, &command) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if kind == "walls" {
        print!("{}", answer["walls"]["stdout"].as_str().unwrap_or_default());
        eprint!("{}", answer["walls"]["stderr"].as_str().unwrap_or_default());
        return answer["walls"]["exit"].as_i64().and_then(|c| i32::try_from(c).ok()).unwrap_or(2);
    }
    // `--checks` in a project with no checks: nothing to run, and nothing failed (M5.34).
    if let Some(none) = answer.get("no_checks").and_then(Value::as_str) {
        println!("{none}");
        return 0;
    }
    let lease = answer["lease"]["id"].as_str().unwrap_or_default().to_owned();
    let io = io_dir();
    let argv: Vec<String> = answer["lease"]["argv"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(|s| s.replace("{io}", &io)).collect())
        .unwrap_or_default();
    let code = run_leased(link, &lease, &argv, None, own);
    if code == 127 && kind == "run" && one_string(&command) {
        eprintln!("mork test: {NOT_ONE_STRING}");
    }
    if code == 126 && kind == "run" {
        eprintln!("mork test: {NOT_A_PROGRAM}");
    }
    code
}

/// Runs a lease's `argv` under a guard, in `cwd` or here, while holding the lease; its exit.
pub(super) fn run_leased(
    mut link: Link,
    lease: &str,
    argv: &[String],
    cwd: Option<&std::path::Path>,
    own: &str,
) -> i32 {
    let Some(program) = argv.first() else { return fail("the lease named no command", 2) };
    // Under a guard leading its group: if this client is killed, the run ends with it (M3.25).
    let mut cmd = super::launch::guard(argv);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = link.ask(json!({"test.done": {"lease": lease, "exit": 127}}));
            return fail(&format!("{program}: {e}"), 127);
        }
    };
    hold(link, lease, Running::Child(child), own)
}

// A `--run` given its whole command as one quoted string, which the shell looks up as a program's name.
const NOT_ONE_STRING: &str = "give the program and its arguments as separate words, e.g. `mork test --run npx jest a.test.js`, not as one quoted string";

// A `--run` whose first word is a file that isn't a program (the shell's 126, "Permission denied"), a test file's path say.
const NOT_A_PROGRAM: &str = "the first word is run as a program: put the program first and the file after it as separate words, e.g. `mork test --run npx jest <file>`";

fn one_string(command: &[String]) -> bool {
    matches!(command, [only] if only.contains(char::is_whitespace))
}

enum Running {
    Child(Child),
    Pid(i32),
}

impl Running {
    fn exited(&mut self) -> Option<i32> {
        match self {
            Self::Child(c) => c.try_wait().ok().flatten().map(|s| s.code().unwrap_or(1)),
            // Our child still, across the exec (1.8.6.5): its real status (M3.24). One that
            // isn't can't be reaped, and its end is no pass.
            Self::Pid(p) => match mork_sys::try_reap(*p) {
                Ok(code) => code,
                Err(_) => (!std::path::Path::new(&format!("/proc/{p}")).exists() || zombie(*p)).then_some(1),
            },
        }
    }
    fn pid(&self) -> i32 {
        match self {
            Self::Child(c) => i32::try_from(c.id()).unwrap_or(0),
            Self::Pid(p) => *p,
        }
    }
    // The guard is sent SIGTERM first, so its passes kill the descendants it adopted, those that left its group
    // included (M4.6, M5.2); after a second, its group is killed.
    fn kill(&mut self) {
        let pid = self.pid();
        if pid > 0 {
            let _ = mork_sys::kill(pid, super::procs::libc_sig::TERM);
            let start = clock::instant();
            let mut ended = false;
            while !ended && clock::elapsed(start) < Duration::from_secs(1) {
                ended = self.exited().is_some();
                clock::sleep(Duration::from_millis(20));
            }
            // The guard leads its group (M3.25); a pid that leads none is ended alone. One that
            // ended and was reaped is signalled no more: its pid may be another's now.
            if !ended {
                let _ = mork_sys::kill(-pid, super::procs::libc_sig::KILL);
                let _ = mork_sys::kill(pid, super::procs::libc_sig::KILL);
            }
        }
        match self {
            Self::Child(c) => {
                let _ = c.wait();
            }
            Self::Pid(p) => {
                let _ = mork_sys::try_reap(*p);
            }
        }
    }
}

fn zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| r.trim_start().starts_with('Z')))
}

// While `child` runs, Mork of another build is handed over to: its `mork test --resume <lease> --child <pid>`,
// exec'd, keeps it our child (1.8.6.5).
fn reclaim(lease: &str, own: &str, child: Option<i32>) -> Option<Link> {
    let start = clock::instant();
    while clock::elapsed(start) < RECONNECT {
        if let Ok(mut l) = connect() {
            if let Some(pid) = child {
                hand_over(&l, lease, own, pid);
            }
            return l.resume(lease, start).then_some(l);
        }
        clock::sleep(Duration::from_millis(300));
    }
    None
}

// Returns only when no other build's binary is there, or its exec failed: this binary carries on (M3.24).
fn hand_over(link: &Link, lease: &str, own: &str, pid: i32) {
    let Some(home) = link.home.as_ref().filter(|_| !link.build.is_empty() && link.build != own) else { return };
    let bin = crate::builds::binary(&crate::paths::Home::new(home.clone()), &link.build);
    if !bin.exists() {
        return;
    }
    let args = ["test".to_owned(), "--resume".into(), lease.into(), "--child".into(), pid.to_string()];
    let err = super::launch::exec(&bin, &args, &[]);
    eprintln!("mork test: couldn't run build {}: {err}", link.build);
}

fn other_build(link: &Link, args: &[String]) -> i32 {
    let home = link.home.clone().map(crate::paths::Home::new);
    let Some(bin) = home.map(|h| crate::builds::binary(&h, &link.build)).filter(|b| b.exists()) else {
        return fail(&format!("Mork runs build {}, whose binary is missing", link.build), 2);
    };
    let mut all = vec!["test".to_owned()];
    all.extend(args.iter().cloned());
    let err = super::launch::exec(&bin, &all, &[]);
    fail(&format!("couldn't run build {}: {err}", link.build), 2)
}

fn hold(mut link: Link, lease: &str, mut running: Running, own: &str) -> i32 {
    let code = loop {
        if let Some(code) = running.exited() {
            break code;
        }
        if !link.open() {
            match reclaim(lease, own, Some(running.pid())) {
                Some(l) => link = l,
                None => {
                    running.kill();
                    return fail(LOST, 3);
                }
            }
        }
        clock::sleep(Duration::from_millis(100));
    };
    let exit = u64::try_from(code).unwrap_or(1);
    let done = json!({"test.done": {"lease": lease, "exit": exit}});
    if !matches!(link.ask(done.clone()), Ok(Ok(_)))
        && let Some(mut l) = reclaim(lease, own, None)
    {
        let _ = l.ask(done);
    }
    code
}

fn resume_cmd(args: &[String], own: &str) -> i32 {
    let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let (Some(lease), Some(pid)) = (after("--resume"), after("--child").and_then(|p| p.parse::<i32>().ok())) else {
        return fail("mork test --resume <lease> --child <pid>", 2);
    };
    let mut running = Running::Pid(pid);
    let since = clock::instant();
    let link = connect().ok().and_then(|mut l| l.resume(&lease, since).then_some(l));
    match link {
        Some(l) => {
            eprintln!("mork test: resumed under build {own}");
            hold(l, &lease, running, own)
        }
        None => {
            running.kill();
            fail(LOST, 3)
        }
    }
}

pub(super) fn fail(words: &(impl std::fmt::Display + ?Sized), code: i32) -> i32 {
    eprintln!("mork test: {words}");
    code
}
