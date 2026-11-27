// Subcommands and options come from the catalog, so the sockets, the window and the command line
// can't drift (1.7.1).

mod task_file;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use mork_core::maintenance::manuals;
use mork_engine::jobs::launch;
use mork_engine::paths::Home;
use mork_types::catalog::{CATALOG, CommandSpec, signature};
use serde_json::{Value, json};

const NO_ANSWER: &str = "no answer: the command may or may not have happened; look before sending it again.";

fn socket(home: &Home) -> PathBuf {
    std::env::var_os("MORK_AGENT_SOCK").filter(|v| !v.is_empty()).map_or_else(|| home.mork_sock(), PathBuf::from)
}

struct Link {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    build: String,
    // From `hello`: inside a wall, the only way to the engine's `builds/` (1.6.4).
    home: Option<PathBuf>,
}

fn connect(home: &Home) -> Result<Link, String> {
    let path = socket(home);
    let stream = UnixStream::connect(&path).map_err(|_| format!("no Mork running on {}", home.root().display()))?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let hello: Value = serde_json::from_str(&line).map_err(|_| "Mork sent no hello".to_owned())?;
    let build = hello["hello"]["build"].as_str().unwrap_or_default().to_owned();
    let home = hello["hello"]["home"].as_str().filter(|h| !h.is_empty()).map(PathBuf::from);
    Ok(Link { stream, reader, build, home })
}

// The running build lives under the engine home advertised in `hello`. A walled caller has its own HOME (1.6.4).
// Returns only on failure.
fn hand_over(home: &Home, link: &Link, args: &[String]) -> i32 {
    let build = link.build.as_str();
    let engine_home = link.home.clone().map_or_else(|| home.clone(), Home::new);
    let bin = mork_engine::builds::binary(&engine_home, build);
    if !bin.exists() {
        eprintln!("Mork runs build {build}, whose binary is missing: {}", bin.display());
        return 2;
    }
    let err = launch::exec(&bin, args, &[]);
    eprintln!("couldn't run build {build}: {err}");
    2
}

/// `mork ctl …`; `args` are everything after `mork`.
pub fn run(home: &Home, own: &str, args: &[String]) -> i32 {
    let rest = args.get(1..).unwrap_or(&[]);
    // `--as orchestrator:<project>`: the command counts as that project's orchestrator's, never the operator's (fac-244).
    let (sent_as, rest) = match rest {
        [flag, who, tail @ ..] if flag == "--as" => (Some(who.as_str()), tail),
        [flag] if flag == "--as" => return usage("mork ctl --as orchestrator:<project> <command>"),
        _ => (None, rest),
    };
    let mut file = None;
    let cmd = match rest.first().map(String::as_str) {
        Some("--json") => match rest.get(1).map(|t| serde_json::from_str::<Value>(t)) {
            Some(Ok(v)) => v,
            _ => return usage("mork ctl --json '<command>'"),
        },
        None | Some("--help" | "help") => return commands(),
        Some("search") => {
            return search(&rest[1..], std::io::stdout().lock()).map_or_else(|e| usage(&e.to_string()), |()| 0);
        }
        Some(name @ ("watch" | "hook")) if !rest.iter().any(|a| a == "--help") => {
            let spec = CATALOG.iter().find(|c| c.name == name);
            return match spec.map(|s| mork_types::words::command(s, &rest[1..])) {
                Some(Ok(cmd)) => watch(home, own, args, &request_line(&cmd, sent_as), name == "hook"),
                Some(Err(e)) => usage(&e),
                None => usage(&format!("mork ctl: unknown command `{name}`")),
            };
        }
        Some("door") if rest.iter().any(|a| a == "--help") => match door_help(&rest[1..]) {
            Some(v) => v,
            None => return 0,
        },
        Some(name) => match CATALOG.iter().find(|c| c.name == name && (!c.faults_only || name == "debug.sleep")) {
            Some(spec) if rest.iter().any(|a| a == "--help") => return help(spec),
            Some(spec) => match words(spec, &rest[1..]) {
                Ok((v, pending)) => {
                    file = pending;
                    v
                }
                Err(e) => return usage(&e),
            },
            None => return usage(&format!("mork ctl: unknown command `{name}`")),
        },
    };
    if cmd.get("wakes").is_some() {
        return wakes(home, own, args, &request_line(&cmd, sent_as));
    }
    let mut link = match connect(home) {
        Ok(l) => l,
        Err(e) => return usage(&e),
    };
    if link.build != own {
        return hand_over(home, &link, args);
    }
    let line = match file {
        Some(p) => match p.read().and_then(|text| p.line(&text, sent_as)) {
            Ok(l) => l,
            Err(e) => return usage(&e),
        },
        None => request_line(&cmd, sent_as),
    };
    send(&mut link, &line)
}

// A command's words; a `spawn --task-file`'s task is read only once the build is known to be ours.
fn words(spec: &CommandSpec, args: &[String]) -> Result<(Value, Option<task_file::Pending>), String> {
    if spec.name == "spawn"
        && let Some(p) = task_file::prepare(spec, args)?
    {
        return Ok((Value::Null, Some(p)));
    }
    mork_types::words::command(spec, args).map(|v| (v, None))
}

fn request_line(cmd: &Value, sent_as: Option<&str>) -> String {
    let mut req = json!({"id": 1, "cmd": cmd});
    if let Some(who) = sent_as {
        req["as"] = json!(who);
    }
    req.to_string()
}

fn exchange(link: &mut Link, line: &str) -> Option<Value> {
    link.stream.write_all(format!("{line}\n").as_bytes()).ok()?;
    loop {
        let mut l = String::new();
        link.reader.read_line(&mut l).ok()?;
        if !l.ends_with('\n') {
            return None;
        }
        let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
        if v.pointer("/waiting/id") == Some(&json!(1)) {
            if let Some(why) = v.pointer("/waiting/why").and_then(Value::as_str) {
                eprintln!("mork ctl: {why}");
            }
            continue;
        }
        // Framing errors (over-limit lines, invalid UTF-8) come back with id 0 (fac-667).
        if v.get("event").is_none() && (v["id"] == 1 || (v["id"] == 0 && v.get("err").is_some())) {
            return Some(v);
        }
    }
}

fn send(link: &mut Link, line: &str) -> i32 {
    let Some(v) = exchange(link, line) else {
        eprintln!("{NO_ANSWER}");
        return 3;
    };
    if let Some(ok) = v.get("ok") {
        println!("{ok}");
        return 0;
    }
    println!("{}", json!({"err": v["err"]}));
    1
}

fn usage(text: &str) -> i32 {
    eprintln!("{text}");
    2
}

// Its signature from the catalog, then its section of the reference (fac-14).
fn help(spec: &CommandSpec) -> i32 {
    match manuals::command_help(spec.name) {
        Some(text) => print!("{text}"),
        None => println!("{}", signature(spec)),
    }
    0
}

fn search(args: &[String], mut out: impl Write) -> std::io::Result<()> {
    if args.iter().any(|a| a == "--help") {
        return write!(out, "{}", manuals::command_help("search").unwrap_or_default());
    }
    let query = args.iter().filter(|a| *a != "--json").map(String::as_str).collect::<Vec<_>>().join(" ");
    if query.trim().is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, manuals::SEARCH_SIGNATURE));
    }
    let hits = manuals::search(&query);
    if args.iter().any(|a| a == "--json") {
        serde_json::to_writer(&mut out, &hits)?;
        writeln!(out)
    } else {
        for hit in hits {
            writeln!(out, "{}\n  {}", hit.signature, hit.snippet)?;
        }
        Ok(())
    }
}

// A build without `faults` lists none of the faults-only commands.
fn commands() -> i32 {
    println!(
        "Every command: its required fields in order, then its optional ones as --<field>; boolean flags take no value; list fields accept comma-separated items or a JSON array of strings in one quoted argument, preserving commas inside items. `mork ctl --json '<command>'` sends one exactly as written; `mork ctl <command> --help` gives its reference."
    );
    for spec in CATALOG.iter().filter(|c| mork_engine::faults::TEST_SETTINGS || !c.faults_only) {
        println!("{}", signature(spec));
    }
    println!("{}", manuals::SEARCH_SIGNATURE);
    0
}

// A target's description is the engine's: `targets.toml` is in no wall.
fn door_help(args: &[String]) -> Option<Value> {
    let target = args.iter().position(|a| a == "--target").and_then(|i| args.get(i + 1));
    match target {
        Some(t) => Some(json!({"door.help": {"target": t}})),
        None => {
            print!("{}", mork_types::door::help());
            None
        }
    }
}

// `watch`, and an orchestrator's `hook`: printed as it comes, again after a restart. A hook replaced
// by another stops, and a refused one too.
fn watch(home: &Home, own: &str, args: &[String], line: &str, hook: bool) -> i32 {
    let mut misses = 0;
    loop {
        let mut link = match connect(home) {
            Ok(l) => l,
            Err(e) => {
                misses += 1;
                if misses > 600 {
                    eprintln!("{e}");
                    return 2;
                }
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        misses = 0;
        if link.build != own {
            return hand_over(home, &link, args);
        }
        if link.stream.write_all(format!("{line}\n").as_bytes()).is_err() {
            continue;
        }
        let mut out = std::io::stdout();
        loop {
            let mut l = String::new();
            match link.reader.read_line(&mut l) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // Mork is still starting; ask again.
                    if l.contains("\"recovering\"") && l.contains("\"err\"") {
                        std::thread::sleep(Duration::from_millis(200));
                        break;
                    }
                    let _ = out.write_all(l.as_bytes());
                    let _ = out.flush();
                    let v: Value = serde_json::from_str(&l).unwrap_or_default();
                    if hook && (v.get("err").is_some() || v.pointer("/event/orchestrator") == Some(&json!("replaced")))
                    {
                        return i32::from(v.get("err").is_some());
                    }
                }
            }
        }
    }
}

fn wakes(home: &Home, own: &str, args: &[String], line: &str) -> i32 {
    loop {
        if let Ok(mut link) = connect(home) {
            if link.build != own {
                return hand_over(home, &link, args);
            }
            match exchange(&mut link, line) {
                Some(v) if v.get("ok").is_some() => return print_wakes(&v["ok"]),
                Some(v) if v.pointer("/err/code").is_some_and(|code| code != "recovering") => {
                    if v["err"]["code"] == "replaced" {
                        eprintln!("{}", v["err"]["message"].as_str().unwrap_or("the `wakes` request was replaced"));
                    } else {
                        println!("{}", json!({"err": v["err"]}));
                    }
                    return 1;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn print_wakes(ok: &Value) -> i32 {
    let entries = ok.get("wakes").and_then(Value::as_array);
    let Some(entries) = entries.and_then(|a| a.iter().map(Value::as_str).collect::<Option<Vec<_>>>()) else {
        return usage("Mork sent an invalid `wakes` answer");
    };
    let mut out = std::io::stdout().lock();
    let written = entries.into_iter().try_for_each(|entry| writeln!(out, "{entry}")).and_then(|()| out.flush());
    match written {
        Ok(()) => 0,
        Err(e) => usage(&format!("couldn't print the wake entries: {e}")),
    }
}

/// One command to a running mork, as the other subcommands send it: `Ok(None)` when no
/// mork runs; `Err` with the refusal's words.
pub fn request(home: &Home, cmd: &Value) -> Result<Option<Value>, String> {
    let Ok(mut link) = connect(home) else { return Ok(None) };
    let line = json!({"id": 1, "cmd": cmd}).to_string();
    let v = exchange(&mut link, &line).ok_or_else(|| NO_ANSWER.to_owned())?;
    match v.get("ok") {
        Some(ok) => Ok(Some(ok.clone())),
        None => Err(v["err"]["message"].as_str().unwrap_or("refused").to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    use serde_json::{Value, json};

    use super::{Link, exchange, search};

    fn search_text(args: &[&str]) -> String {
        let mut output = Vec::new();
        let args: Vec<_> = args.iter().map(|s| s.to_string()).collect();
        search(&args, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn search_outputs_the_signature_and_matching_help_in_both_formats() {
        let hits: Vec<Value> = serde_json::from_str(&search_text(&["PAUSE --agent", "--json"])).unwrap();
        let stop = hits.iter().find(|hit| hit["signature"].as_str().unwrap().starts_with("mork ctl stop ")).unwrap();
        assert_eq!(stop["signature"], "mork ctl stop [--card <a string>] [--agent <a string>]");
        assert!(stop["snippet"].as_str().unwrap().contains("its agents pause"));
        let plain: String = hits
            .iter()
            .map(|hit| format!("{}\n  {}\n", hit["signature"].as_str().unwrap(), hit["snippet"].as_str().unwrap()))
            .collect();
        assert_eq!(search_text(&["--agent", "pause"]), plain);
    }

    #[test]
    fn spawn_help_shows_the_two_task_forms_on_its_first_line() {
        let help = mork_core::maintenance::manuals::command_help("spawn").unwrap();
        assert!(help.lines().next().unwrap().contains("(<task: a string> | --task-file <path, or - for stdin>)"));
    }

    #[test]
    fn search_signature_match_uses_summary_and_all_words_are_required() {
        let help = search_text(&["--help"]);
        assert_eq!(search_text(&["merge", "--help", "--json"]), help);
        assert_eq!(search_text(&["--help", "merge"]), help);
        let (signature, body) = help.split_once("\n\n").unwrap();
        let summary = body.lines().next().unwrap();
        assert_eq!(search_text(&["<QUERY>"]), format!("{signature}\n  {summary}\n"));
        assert_eq!(search_text(&["<QUERY>", "absent-string-1234"]), "");
        assert_eq!(search_text(&["<QUERY>", "absent-string-1234", "--json"]), "[]\n");
        assert!(search(&["--json".into()], Vec::new()).is_err());
    }

    #[test]
    fn exchange_returns_id_zero_engine_errors() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let reader = BufReader::new(client.try_clone().unwrap());
        let mut link = Link { stream: client, reader, build: String::new(), home: None };
        let peer = std::thread::spawn(move || {
            let mut req = String::new();
            BufReader::new(&server).read_line(&mut req).unwrap();
            let lines = [
                json!({"event": {"kind": "tick"}}),
                json!({"id": 0, "ok": {}}),
                json!({"id": 0, "err": {"code": "too_large", "message": "the line is over 1 MiB"}}),
            ];
            for line in lines {
                writeln!(server, "{line}").unwrap();
            }
        });
        let answer = exchange(&mut link, r#"{"id":1,"cmd":{"status":{}}}"#).unwrap();
        peer.join().unwrap();
        assert_eq!(answer["id"], 0);
        assert_eq!(answer["err"]["code"], "too_large");
        assert_eq!(answer["err"]["message"], "the line is over 1 MiB");
    }
}
