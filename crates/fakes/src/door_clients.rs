//! Local clients of the door (fac-86): each reads its login from the option file in the call's own folder,
//! never argv; a socket client checks the forward first, so a dropped forward is a failure the engine can tell.

use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::door::{after, fail, log, option, scenario, stdin_all};

// A value from a MySQL option file: `key=value`, the value double-quoted as MySQL reads it back.
fn cnf_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| unquote(v))
    })
}

// A value from a YAML config of one key per line: `key: value`, the value double-quoted.
fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| unquote(v))
    })
}

// A double-quoted value with its backslash escapes undone; an unquoted one as it is.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) else { return s.to_owned() };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out
}

// Every `%XX` of a percent-encoded value undone: a MongoDB URI's host is one.
fn percent_decode(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(x) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(x);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// A MongoDB URI's host, decoded: between the login's `@` and the next `/`.
fn uri_host(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("mongodb://")?;
    let after_at = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
    let host = after_at.split(['/', '?']).next().unwrap_or("");
    Some(percent_decode(host))
}

// Whether a forward's socket is there and alive: a byte comes back on it. A dropped forward refuses
// the connection or shows its end at once.
fn socket_alive(path: &str) -> bool {
    let Ok(mut s) = UnixStream::connect(path) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut b = [0u8; 1];
    matches!(s.read(&mut b), Ok(1))
}

fn reply(kind: &str, input: &str) -> Value {
    scenario()[kind]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["match"].as_str().is_none_or(|m| input.contains(m))).cloned())
        .unwrap_or(Value::Null)
}

fn play(r: &Value, default: &str, pw: &str) -> i32 {
    let mut out = r["out"].as_str().map(str::to_owned).unwrap_or_default();
    if let Some(f) = r["out_file"].as_str() {
        out = std::fs::read_to_string(f).unwrap_or_default();
    }
    if let Some(n) = r["rows"].as_u64() {
        out = std::iter::once("n\tpad".to_owned())
            .chain((0..n).map(|i| format!("{i}\t{}", "p".repeat(40))))
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
    }
    if out.is_empty() && r["exit"].as_i64().unwrap_or(0) == 0 {
        out = default.to_owned();
    }
    match r["password"].as_str() {
        Some("out") => out.push_str(&format!("9\t{pw}\n")),
        Some("err") => eprintln!("ERROR 1045 (28000): Access denied (using password: {pw})"),
        _ => {}
    }
    print!("{out}");
    let _ = std::io::stdout().flush();
    if let Some(e) = r["err"].as_str() {
        eprint!("{e}");
    }
    i32::try_from(r["exit"].as_i64().unwrap_or(0)).unwrap_or(1)
}

/// `mysql --defaults-file=<options> --batch --database=<db>`, the query on stdin; its login and
/// its server (a forward's socket, or an address) read from the options file, never its argv.
pub fn mysql_main() -> i32 {
    let argv: Vec<String> = std::env::args().collect();
    // The script is read as it comes: the session's id first, the rest only once that is answered.
    let mut stdin = Vec::new();
    let mut input = std::io::stdin().lock();
    let _ = input.read_until(b'\n', &mut stdin);
    let connected = mysql_connect(&argv);
    let session = connected.is_ok() && stdin == b"SELECT CONNECTION_ID();\n";
    if session {
        print!("CONNECTION_ID()\n{}\n", std::process::id());
        let _ = std::io::stdout().flush();
    }
    let _ = input.read_to_end(&mut stdin);
    log("mysql", &argv, &stdin);
    let text = match connected {
        Ok(text) => text,
        Err(code) => return code,
    };
    if let Some(sql) = after(&argv, "-e") {
        return kill_session(sql);
    }
    let (_, query, _) = framed(&String::from_utf8_lossy(&stdin));
    if argv.iter().any(|a| a.starts_with("--init-command="))
        && let Some(error) = reply("mysql", &query)["init_error"].as_str()
    {
        return fail(1, error);
    }
    let script = String::from_utf8_lossy(&stdin).into_owned();
    let input = if session { &script["SELECT CONNECTION_ID();\n".len()..] } else { &script };
    mysql_read(input, &script, &text, argv.iter().any(|a| a == "--force"))
}

// Its option file's text, or the code it ends with when it can't connect.
fn mysql_connect(argv: &[String]) -> Result<String, i32> {
    let Some(cred) = argv.get(1).and_then(|a| a.strip_prefix("--defaults-file=")) else {
        return Err(fail(7, "mysql: --defaults-file must come first"));
    };
    let text = std::fs::read_to_string(cred).unwrap_or_default();
    if cnf_value(&text, "protocol").as_deref() == Some("SOCKET")
        && let Some(sock) = cnf_value(&text, "socket")
        && !socket_alive(&sock)
    {
        return Err(fail(2, "ERROR 2002 (HY000): Can't connect to MySQL server: its forward is gone"));
    }
    if after(argv, "-e").is_none() && (!Path::new(cred).is_file() || !argv.iter().any(|a| a == "--batch")) {
        return Err(fail(1, "ERROR 2002 (HY000): no credential, or not batch"));
    }
    Ok(text)
}

fn mysql_read(input: &str, script: &str, options: &str, force: bool) -> i32 {
    let (before, query, after) = framed(input);
    let mut r = reply("mysql", &query);
    for s in &before {
        if mysql_optional_error(s, &r, script) && !force {
            return 1;
        }
    }
    let profiling = r["profiling"] != false;
    let on = profiling
        && r["prepare_error"].is_null()
        && before.iter().any(|s| matches!(*s, "SET profiling = 1" | "EXECUTE mork_on"));
    if let Some(e) = r["err"].as_str() {
        r["err"] = json!(e.replace("{line}", &mysql_line(script, &query).to_string()));
    }
    let started = Instant::now();
    let code = play(&r, "id\tname\n1\talpha\n2\tbeta\n", &cnf_value(options, "password").unwrap_or_default());
    if code != 0 && !force {
        return code;
    }
    let profile = vec![(query, started.elapsed().as_secs_f64())];
    trailer(&after, profile, on, &r, script, force)
}

fn mysql_line(script: &str, statement: &str) -> usize {
    script.lines().position(|s| s.strip_suffix(';').unwrap_or(s) == statement).map_or(0, |n| n + 1)
}

// Batch mode aborts before the read on a setup error unless --force was actually passed.
fn mysql_optional_error(statement: &str, r: &Value, script: &str) -> bool {
    let error = if statement.starts_with("PREPARE mork_") {
        r["prepare_error"].as_str()
    } else if statement.starts_with("EXECUTE mork_") && r["prepare_error"].is_string() {
        Some("ERROR 1243 (HY000): Unknown prepared statement handler")
    } else if matches!(statement, "SHOW PROFILES" | "EXECUTE mork_show") {
        r["collect_error"].as_str().or_else(|| {
            (statement == "SHOW PROFILES" && r["profiling"] == false)
                .then_some("ERROR 1289 (HY000): The 'SHOW PROFILES' feature is disabled")
        })
    } else {
        (statement == "SET profiling = 1" && r["profiling"] == false)
            .then_some("ERROR 1193 (HY000): Unknown system variable 'profiling'")
    };
    if let Some(error) = error {
        let (head, words) = error.split_once(": ").unwrap_or((error, ""));
        fail(1, &format!("{head} at line {}: {words}", mysql_line(script, statement)));
    }
    error.is_some()
}

// --force continues through collection errors too, and can exit zero after a failed read.
fn trailer(after: &[&str], mut profile: Vec<(String, f64)>, on: bool, r: &Value, script: &str, force: bool) -> i32 {
    for s in after {
        if mysql_optional_error(s, r, script) {
            if !force {
                return 1;
            }
        } else if let Some(end) = s.strip_prefix("SELECT '").and_then(|x| x.strip_suffix("' AS mork_end")) {
            print!("mork_end\n{end}\n");
            profile.push(((*s).to_owned(), 0.00002));
        } else if on && matches!(*s, "SHOW PROFILES" | "EXECUTE mork_show") {
            println!("Query_ID\tDuration\tQuery");
            for (n, (q, secs)) in profile.iter().enumerate() {
                println!("{}\t{secs:.8}\t{}", n + 1, q.get(..300).unwrap_or(q));
            }
        }
    }
    0
}

// A session's statements, one a line: its own before the query (profiling), the query, and its own
// after it (the query's boundary, the profile).
fn framed(text: &str) -> (Vec<&str>, String, Vec<&str>) {
    let lines: Vec<&str> = text.lines().map(|l| l.strip_suffix(';').unwrap_or(l)).collect();
    let own = |s: &&&str| {
        [
            "SET SESSION sql_select_limit",
            "SET @mork_",
            "PREPARE mork_",
            "EXECUTE mork_",
            "SET profiling",
            "SHOW PROFILES",
        ]
        .iter()
        .any(|p| s.starts_with(p))
            || s.ends_with("' AS mork_end")
    };
    let lead = lines.iter().take_while(own).count();
    let tail = lines[lead..].iter().rev().take_while(own).count();
    let query = lines[lead..lines.len() - tail].join("\n");
    (lines[..lead].to_vec(), query, lines[lines.len() - tail..].to_vec())
}

// No session is simulated, so no thread id is ever known.
fn kill_session(sql: &str) -> i32 {
    let rest = sql.strip_prefix("KILL CONNECTION ").or_else(|| sql.strip_prefix("KILL QUERY ")).unwrap_or("");
    let Some(id) = rest.trim().parse::<u32>().ok() else { return fail(1, "ERROR 1064 (42000): invalid kill") };
    fail(1, &format!("ERROR 1094 (HY000): Unknown thread id: {id}"))
}

/// `mongoexport --config=<options> --db --collection --query --limit …`; its login and server read
/// from the config, the forward's socket (if any) checked first.
pub fn mongoexport_main() -> i32 {
    let argv: Vec<String> = std::env::args().collect();
    log("mongoexport", &argv, &stdin_all());
    let Some(cred) = option(&argv, "--config=").filter(|c| Path::new(c).is_file()) else {
        return fail(1, "mongoexport: no --config");
    };
    let text = std::fs::read_to_string(cred).unwrap_or_default();
    if let Some(host) = yaml_value(&text, "uri").and_then(|u| uri_host(&u))
        && host.starts_with('/')
        && !socket_alive(&host)
    {
        return fail(1, "mongoexport: can't connect through the forward's socket: it is gone");
    }
    if option(&argv, "--query=").is_none_or(|q| serde_json::from_str::<Value>(q).is_err()) {
        return fail(1, "mongoexport: --query isn't JSON");
    }
    let collection = option(&argv, "--collection=").unwrap_or("").to_owned();
    let pw = yaml_value(&text, "password").unwrap_or_default();
    let r = reply("mongoexport", &collection);
    if r["password"].as_str() == Some("out") {
        println!("{}", json!({"_id": 9, "uri_password": pw}));
    }
    play(
        &json!({"out": r["out"], "out_file": r["out_file"], "exit": r["exit"], "err": r["err"]}),
        "{\"_id\":1,\"status\":500}\n{\"_id\":2,\"status\":502}\n",
        &pw,
    )
}
