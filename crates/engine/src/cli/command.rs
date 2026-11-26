//! Shell labels shared by the CLI streams. A protected message has no inspectable command.

use std::borrow::Cow;
use std::path::Path;

use mork_types::seen::CommandView;

use super::{said, turn};

pub(super) fn display(seat: Option<&str>, raw: &str, description: Option<&str>) -> (String, Option<CommandView>) {
    let line = unwrap_shell(raw.trim());
    let masked = said::words(seat, raw).or_else(|| (line != raw).then(|| said::words(seat, &line)).flatten());
    if let Some(words) = masked {
        return (words, None);
    }
    let words = description
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .or_else(|| label(line.trim()))
        .unwrap_or_else(|| line.trim().to_owned());
    let (text, cut) = turn::cut_to(raw, turn::DETAIL_MAX);
    (words.lines().next().unwrap_or_default().to_owned(), Some(CommandView { text, cut }))
}

fn unwrap_shell(line: &str) -> Cow<'_, str> {
    let shell = ["/bin/bash ", "bash ", "/bin/sh ", "sh "].iter().find_map(|s| line.strip_prefix(s));
    let Some(body) = shell.and_then(|s| s.strip_prefix("-lc ").or_else(|| s.strip_prefix("-c "))) else {
        return line.into();
    };
    let body = body.trim();
    if body.starts_with(['\'', '"'])
        && let Some(mut args) = shlex::split(body)
        && args.len() == 1
    {
        return args.remove(0).into();
    }
    body.into()
}

// A label for a single invocation could conceal a pipeline, expansion or redirect.
fn simple(line: &str) -> bool {
    let mut quote = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), _) => {}
            (_, '\\') => {
                chars.next();
            }
            (_, '`' | '$') => return false,
            (None, '\'' | '"') => quote = Some(c),
            (None, '|' | '&' | ';' | '<' | '>' | '(' | ')' | '\n') => return false,
            _ => {}
        }
    }
    quote.is_none()
}

fn label(line: &str) -> Option<String> {
    if !simple(line) {
        return None;
    }
    let words = shlex::split(line)?;
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["cat", paths @ ..] => {
            let paths = paths.strip_prefix(&["--"]).unwrap_or(paths);
            let names = paths.iter().map(|p| file_name(p)).collect::<Option<Vec<_>>>()?;
            (!names.is_empty()).then(|| format!("Read {}", names.join(", ")))
        }
        ["sed", "-n", range, path] => sed_range(range, path),
        ["rg" | "grep", args @ ..] => search(args),
        ["git", "status", ..] => Some("Check working tree status".into()),
        ["git", "diff", rest @ ..] => {
            let mut flags = rest.iter().take_while(|w| **w != "--");
            let stat = flags.any(|w| *w == "--stat" || w.starts_with("--stat="));
            Some(if stat { "Summarize file changes" } else { "Show file changes" }.into())
        }
        ["mork", "ctl", "card.show", card] if !card.starts_with('-') => Some("Read card notes".into()),
        ["mork", "ctl", "memory.search", words @ ..] if !words.is_empty() && !words[0].starts_with('-') => {
            Some(format!("Search memory for {}", words.join(" ")))
        }
        ["mork", "ctl", "memory.read", memory, name] => Some(format!("Read {name} from {memory} memory")),
        _ => None,
    }
}

fn file_name(path: &str) -> Option<&str> {
    if path.starts_with('-') {
        return None;
    }
    Path::new(path).file_name()?.to_str()
}

fn sed_range(range: &str, path: &str) -> Option<String> {
    let range = range.strip_suffix('p')?;
    let (first, last) = range.split_once(',').unwrap_or((range, range));
    let first: u64 = first.parse().ok()?;
    let last: u64 = last.parse().ok()?;
    let lines = if first == last { first.to_string() } else { format!("{first}-{last}") };
    Some(format!("Read {} ({lines})", file_name(path)?))
}

fn search(args: &[&str]) -> Option<String> {
    let mut args = args.iter().copied().peekable();
    while let Some(flag) = args.next_if(|w| w.starts_with('-')) {
        match flag {
            "--" | "-e" | "--regexp" => break,
            "-g" | "--glob" | "-t" | "--type" | "-A" | "-B" | "-C" => {
                args.next()?;
            }
            "--line-number" | "--hidden" | "--fixed-strings" | "--ignore-case" | "--no-heading" => {}
            _ if flag.len() > 1 && flag[1..].chars().all(|c| "niIrRFwlHhsS".contains(c)) => {}
            _ => return None,
        }
    }
    let pattern = args.next()?;
    let paths: Vec<_> = args.collect();
    if paths.iter().any(|p| p.starts_with('-')) {
        return None;
    }
    Some(if paths.is_empty() {
        format!("Search {pattern}")
    } else {
        format!("Search {pattern} in {}", paths.join(", "))
    })
}
