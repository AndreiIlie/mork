// `spawn --task-file`: no shell quoting touches the task. The words form alone knows it: the
// catalog, the JSON form and the window don't.

use std::io::Read;

use mork_types::catalog::{CommandSpec, FieldType};
use mork_types::protocol::MAX_LINE;
use serde_json::{Value, json};

const BOTH: &str = "spawn takes its task in words or from --task-file, not both";
const USAGE: &str = "usage: mork ctl spawn <card> <role> --task-file <path, or - for stdin> [--model …] [--effort …]";

fn refused(why: impl std::fmt::Display) -> String {
    format!("{why}\n{USAGE}")
}

/// A spawn whose task is still to be read: its words checked, the task a placeholder.
pub struct Pending {
    path: String,
    cmd: Value,
}

/// Finds `--task-file` among `spawn`'s words and checks the rest, reading nothing: the build the
/// engine runs reads it, after the handover.
pub fn prepare(spec: &CommandSpec, args: &[String]) -> Result<Option<Pending>, String> {
    checked(spec, args).map_err(refused)
}

fn checked(spec: &CommandSpec, args: &[String]) -> Result<Option<Pending>, String> {
    let Some((path, rest)) = split(spec, args)? else { return Ok(None) };
    // Words that already make a whole spawn carry a task of their own.
    if mork_types::words::command(spec, &rest).is_ok() {
        return Err(BOTH.into());
    }
    // In front, so the placeholder never fills a dangling option at the end.
    let named: Vec<String> = ["--task".to_owned(), String::new()].into_iter().chain(rest).collect();
    let cmd = mork_types::words::command(spec, &named)?;
    Ok(Some(Pending { path, cmd }))
}

// `--task-file`'s path and the other words, read by position as the words parser reads them, so
// an option's value is never taken for an option.
fn split(spec: &CommandSpec, args: &[String]) -> Result<Option<(String, Vec<String>)>, String> {
    let (mut path, mut rest, mut i, mut task) = (None, Vec::new(), 0, false);
    while let Some(a) = args.get(i) {
        if a == "--task-file" {
            if path.is_some() {
                return Err("name --task-file once".into());
            }
            path = Some(args.get(i + 1).ok_or("--task-file needs a path, or - for stdin")?.clone());
            i += 2;
            continue;
        }
        task |= a == "--task";
        let field = a.strip_prefix("--").and_then(|n| spec.fields.iter().find(|f| f.name == n));
        let width = if field.is_some_and(|f| f.ty != FieldType::Bool) { 2 } else { 1 };
        rest.extend(args.iter().skip(i).take(width).cloned());
        i += width;
    }
    if task && path.is_some() {
        return Err(BOTH.into());
    }
    Ok(path.map(|p| (p, rest)))
}

impl Pending {
    /// The task's text, read once: the file, or stdin for `-`.
    pub fn read(&self) -> Result<String, String> {
        let mut text = String::new();
        let read = if self.path == "-" {
            std::io::stdin().read_to_string(&mut text).map(|_| ())
        } else {
            std::fs::read_to_string(&self.path).map(|t| text = t)
        };
        read.map_err(|e| refused(format!("couldn't read the task from {}: {e}", self.path)))?;
        Ok(text)
    }

    /// The request line with `text` as its task, refused when the engine would refuse its size.
    pub fn line(mut self, text: &str, sent_as: Option<&str>) -> Result<String, String> {
        let task = text.trim_end_matches(['\n', '\r']);
        if task.trim().is_empty() {
            return Err(refused(format!("the task in {} is empty", self.path)));
        }
        self.cmd["spawn"]["task"] = json!(task);
        let line = super::request_line(&self.cmd, sent_as);
        // Refuse before sending: the engine rejects lines over MAX_LINE.
        if line.len() + 1 > MAX_LINE {
            let size = line.len() + 1;
            return Err(refused(format!("the spawn request is {size} bytes, over Mork's {MAX_LINE}-byte line")));
        }
        Ok(line)
    }
}

#[cfg(test)]
mod tests {
    use mork_types::catalog::CATALOG;
    use mork_types::protocol::MAX_LINE;
    use serde_json::{Value, json};

    use super::{BOTH, Pending, prepare};

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    fn pending(line: &str) -> Result<Option<Pending>, String> {
        let spec = CATALOG.iter().find(|c| c.name == "spawn").ok_or("no spawn")?;
        prepare(spec, &words(line))
    }

    #[test]
    fn the_file_text_is_the_task_verbatim_but_its_trailing_newlines() {
        let p = pending("fac-1 researcher --model sol --effort xhigh --task-file task.md").unwrap().unwrap();
        let text = "Read `a` and (b), it's \"quoted\".\n\n";
        let line: Value = serde_json::from_str(&p.line(text, None).unwrap()).unwrap();
        let want = json!({"card": "fac-1", "role": "researcher", "model": "sol", "effort": "xhigh",
                          "task": "Read `a` and (b), it's \"quoted\"."});
        assert_eq!(line["cmd"]["spawn"], want);
    }

    #[test]
    fn a_task_in_words_beside_the_file_is_refused() {
        assert!(pending("fac-1 researcher look here --task-file -").is_err_and(|e| e.starts_with(BOTH)));
        assert!(pending("fac-1 researcher --task look --task-file -").is_err_and(|e| e.starts_with(BOTH)));
        assert!(pending("fac-1 researcher --task-file").is_err());
        assert!(pending("fac-1 researcher look here").unwrap().is_none());
        assert!(pending("pr-ex-12 reviewer --topic correctness --task review").unwrap().is_none());
        assert!(pending("fac-1 researcher --task-file - --task look").is_err_and(|e| e.starts_with(BOTH)));
        assert!(
            pending("pr-ex-12 reviewer --task-file - --topic").is_err_and(|e| e.starts_with("--topic needs a value"))
        );
    }

    #[test]
    fn an_option_value_that_looks_like_an_option_is_kept() {
        let p = pending("pr-ex-12 reviewer --topic --task-file --task-file -").unwrap().unwrap();
        let line: Value = serde_json::from_str(&p.line("review it", None).unwrap()).unwrap();
        assert_eq!(line["cmd"]["spawn"]["topic"], json!("--task-file"));
        assert_eq!(line["cmd"]["spawn"]["task"], json!("review it"));
    }

    #[test]
    fn an_empty_text_is_refused() {
        let p = pending("fac-1 researcher --task-file -").unwrap().unwrap();
        assert!(p.line("\n\n", None).is_err());
    }

    #[test]
    fn a_request_over_the_line_limit_is_refused() {
        let p = pending("fac-1 researcher --task-file -").unwrap().unwrap();
        // Each quote takes two bytes once escaped: half the limit in quotes passes it.
        assert!(p.line(&"\"".repeat(MAX_LINE / 2), None).is_err());
        let p = pending("fac-1 researcher --task-file -").unwrap().unwrap();
        assert!(p.line(&"a".repeat(MAX_LINE / 2), None).is_ok());
    }
}
