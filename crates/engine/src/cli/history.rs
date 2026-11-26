//! Whether a saved session holds its retry's request (fac-626). Read beneath its own homes without following links.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use mork_types::config::Cli;
use serde_json::Value;

// Past all but 2 of 4,860 saved sessions seen, a first turn's far smaller: a larger one, or one grown past it as
// it is read, proves nothing.
const MAX_BYTES: u64 = 64 << 20;

// No first turn's prompt is near this long; a longer one is not looked for.
const PROMPT_MAX: usize = 1 << 20;

// A line holding the prompt is at most 6 bytes a byte of it (a control character's `\u00XX`) and this much more.
const LINE_ROOM: usize = 128 << 10;

// Claude shortens a longer folder's name with a hash of its own; such a folder is not looked for.
const CLAUDE_NAME_MAX: usize = 200;

// A dated folder is listed this far only (the busiest day seen held 165 sessions).
const ENTRIES_MAX: usize = 4096;

const DAY_MS: i64 = 86_400_000;

// Both CLIs' compaction lines name it: Claude's `compact_boundary`, Codex's `compacted`.
const COMPACT: &[u8] = b"compact";

const CUT_SHORT: &str = "its saved session ends mid-line";
const TOO_LARGE: &str = "its saved session is too large to read";

/// Whether `cli`'s saved session `session`, under `env`'s homes for the folder `cwd`, holds `prompt` as one
/// whole message of the user's with no compaction after it; else why not, in words that quote nothing of it.
pub fn holds(cli: Cli, env: &BTreeMap<String, String>, cwd: &str, session: &str, prompt: &str) -> Result<(), String> {
    if !id(session) || prompt.is_empty() {
        return Err("no session to read".into());
    }
    if prompt.len() > PROMPT_MAX {
        return Err("its prompt is too long to look for".into());
    }
    let file = match cli {
        Cli::Claude => claude_file(env, cwd, session)?,
        Cli::Codex => codex_file(env, session)?,
        // Its conversations are kept in a form of its own, unread here: a retry sends its whole request.
        Cli::Agy => return Err("agy's saved conversations aren't read".into()),
    };
    if scan(file, prompt, cli)? {
        Ok(())
    } else {
        Err("its session doesn't hold its prompt whole since any compaction".into())
    }
}

fn id(session: &str) -> bool {
    !session.is_empty() && session.len() <= 64 && session.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn home(env: &BTreeMap<String, String>, key: &str) -> Option<PathBuf> {
    env.get(key).filter(|h| !h.is_empty()).map(PathBuf::from)
}

// `<config>/projects/<folder, each character but a letter or digit a dash>/<session>.jsonl`.
fn claude_file(env: &BTreeMap<String, String>, cwd: &str, session: &str) -> Result<File, String> {
    let (base, rel) = match home(env, "CLAUDE_CONFIG_DIR") {
        Some(dir) => (dir, PathBuf::from("projects")),
        None => (home(env, "HOME").ok_or("no home")?, PathBuf::from(".claude/projects")),
    };
    let name: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    if !cwd.is_ascii() || name.len() > CLAUDE_NAME_MAX {
        return Err("its folder's saved sessions aren't looked for".into());
    }
    open(&base, &rel.join(name).join(format!("{session}.jsonl")))
}

// `<codex home>/sessions/<year>/<month>/<day>/rollout-<time>-<thread>.jsonl`, the local day the thread began:
// its id's UTC day, the one before or the one after. Only those three folders are read.
fn codex_file(env: &BTreeMap<String, String>, thread: &str) -> Result<File, String> {
    let (base, rel) = match home(env, "CODEX_HOME") {
        Some(dir) => (dir, PathBuf::from("sessions")),
        None => (home(env, "HOME").ok_or("no home")?, PathBuf::from(".codex/sessions")),
    };
    let began = began_ms(thread).ok_or("its thread's id doesn't say when it began")?;
    let root = File::open(&base).map_err(|e| format!("its home: {e}"))?;
    let suffix = format!("-{thread}.jsonl");
    for day in [0, 1, -1] {
        let (y, m, d, ..) = mork_core::state::names::civil_from_ms(began.saturating_add(day * DAY_MS));
        let dated = rel.join(format!("{y:04}/{m:02}/{d:02}"));
        let Ok(dir) = mork_sys::open_dir_beneath(&root, &dated) else { continue };
        let names = mork_sys::read_dir_of(&dir).into_iter().flatten().take(ENTRIES_MAX).flatten();
        let mut names = names.filter_map(|e| e.file_name().into_string().ok());
        if let Some(name) = names.find(|n| n.starts_with("rollout-") && n.ends_with(&suffix)) {
            return open(&base, &dated.join(name));
        }
    }
    Err("no saved session".into())
}

// A version 7 uuid begins with its making's time, in milliseconds since the epoch.
fn began_ms(thread: &str) -> Option<i64> {
    let v7 = thread.len() == 36 && thread.as_bytes().get(14) == Some(&b'7');
    let hex: String = thread.get(..13).filter(|_| v7)?.chars().filter(|c| *c != '-').collect();
    i64::from_str_radix(&hex, 16).ok().filter(|_| hex.len() == 12)
}

// A seat may plant a link or a FIFO in its home: neither is followed or waited on.
fn open(base: &Path, rel: &Path) -> Result<File, String> {
    let file = mork_sys::open_beneath(base, rel).map_err(|e| format!("no saved session: {e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("its saved session isn't a regular file".into());
    }
    if meta.len() > MAX_BYTES {
        return Err(TOO_LARGE.into());
    }
    Ok(file)
}

#[derive(PartialEq, Eq)]
enum Mark {
    Prompt,
    Compaction,
    Other,
}

// Read to its proven end within MAX_BYTES; a read failing, a line cut short or the cap reached proves nothing.
fn scan(file: File, prompt: &str, cli: Cli) -> Result<bool, String> {
    let keep = prompt.len() * 6 + LINE_ROOM;
    let mut reader = BufReader::new(file.take(MAX_BYTES + 1));
    let (mut line, mut held) = (Line::default(), false);
    while next_line(&mut reader, keep, &mut line)? {
        let mark = if line.fits {
            whole(cli, &line, prompt)?
        } else if line.named {
            Mark::Compaction
        } else {
            Mark::Other
        };
        match mark {
            Mark::Prompt => held = true,
            Mark::Compaction => held = false,
            Mark::Other => {}
        }
    }
    if reader.get_ref().limit() == 0 { Err(TOO_LARGE.into()) } else { Ok(held) }
}

// Only a line as long as the prompt can hold it, and only one naming a compaction can be one.
fn whole(cli: Cli, line: &Line, prompt: &str) -> Result<Mark, String> {
    if line.kept.len() < prompt.len() && !line.named {
        return Ok(Mark::Other);
    }
    let v: Value =
        serde_json::from_slice(&line.kept).map_err(|_| "a line of its saved session isn't JSON".to_owned())?;
    Ok(mark(cli, &v, prompt))
}

// Codex saves a prompt's images in its record as data URLs: each one's payload is dropped as it is read.
const DATA_URL: &[u8] = b"\"image_url\":\"data:";

#[derive(Default)]
struct Line {
    // The line, each image's payload dropped, while it is no longer than one holding the prompt can be.
    kept: Vec<u8>,
    fits: bool,
    // Anywhere in it, `compact`: it may be a compaction.
    named: bool,
    in_data: bool,
    tail: Vec<u8>,
}

// The next line, read to its end a buffer at a time; false at the file's end, an error at its end mid-line.
fn next_line(reader: &mut impl BufRead, keep: usize, line: &mut Line) -> Result<bool, String> {
    (line.fits, line.named, line.in_data) = (true, false, false);
    line.kept.clear();
    line.tail.clear();
    let mut started = false;
    loop {
        let buf = reader.fill_buf().map_err(|e| e.to_string())?;
        if buf.is_empty() {
            return if started { Err(CUT_SHORT.into()) } else { Ok(false) };
        }
        started = true;
        let end = buf.iter().position(|b| *b == b'\n');
        let part = &buf[..end.map_or(buf.len(), |e| e + 1)];
        line.tail.extend_from_slice(part);
        line.named |= names(&line.tail);
        line.tail.drain(..line.tail.len().saturating_sub(COMPACT.len()));
        if line.fits {
            line.keep(part, keep);
        }
        let used = part.len();
        reader.consume(used);
        if end.is_some() {
            return Ok(true);
        }
    }
}

impl Line {
    // A data URL runs to its closing quote; any other character in it gives the line up rather than guess.
    fn keep(&mut self, mut part: &[u8], keep: usize) {
        while self.fits && !part.is_empty() {
            if self.in_data {
                let n = part.iter().position(|b| !in_data_url(*b)).unwrap_or(part.len());
                part = &part[n..];
                match part.first() {
                    Some(b'"') => self.in_data = false,
                    Some(_) => self.fits = false,
                    None => {}
                }
            } else {
                let n = part.iter().position(|b| *b == b':').map_or(part.len(), |i| i + 1);
                self.kept.extend_from_slice(&part[..n]);
                part = &part[n..];
                self.in_data = self.kept.ends_with(DATA_URL);
                self.fits = self.kept.len() <= keep;
            }
        }
    }
}

fn in_data_url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"+/=;,.-_".contains(&b)
}

fn names(bytes: &[u8]) -> bool {
    bytes.windows(COMPACT.len()).any(|w| w == COMPACT)
}

fn mark(cli: Cli, v: &Value, prompt: &str) -> Mark {
    let text = |p: &str| v.pointer(p).and_then(Value::as_str);
    match cli {
        Cli::Claude if text("/type") == Some("system") && text("/subtype") == Some("compact_boundary") => {
            Mark::Compaction
        }
        Cli::Claude => {
            let main = v.get("isSidechain").and_then(Value::as_bool) != Some(true);
            let user = text("/type") == Some("user") && text("/message/role") == Some("user");
            if main && user && text("/message/content") == Some(prompt) { Mark::Prompt } else { Mark::Other }
        }
        Cli::Codex if text("/type") == Some("compacted") => Mark::Compaction,
        Cli::Codex => {
            let user = text("/type") == Some("response_item")
                && text("/payload/type") == Some("message")
                && text("/payload/role") == Some("user");
            let parts = v.pointer("/payload/content").and_then(Value::as_array).filter(|_| user);
            let whole = |p: &Value| {
                p.get("type").and_then(Value::as_str) == Some("input_text")
                    && p.get("text").and_then(Value::as_str) == Some(prompt)
            };
            if parts.is_some_and(|ps| ps.iter().any(whole)) { Mark::Prompt } else { Mark::Other }
        }
        Cli::Agy => Mark::Other,
    }
}
