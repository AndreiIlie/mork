//! A command typed as words (`start hf-550 report`): `factory ctl`'s subcommands and the window's
//! `/` lines read them here, one parser for both (1.7, 5.10.2, 1.12.6).

use serde_json::{Map, Value, json};

use crate::catalog::{CommandSpec, Field, FieldType};

fn value(ty: FieldType, raw: &[String], name: &str) -> Result<Value, String> {
    match ty {
        FieldType::Str | FieldType::StrOrList => Ok(json!(raw.join(" "))),
        // The whole selection is a number only if all of it is: `2 checks pass` is a task's text.
        FieldType::Pick => {
            let all = raw.join(" ");
            Ok(all.parse::<u64>().map_or_else(|_| json!(all), Value::from))
        }
        FieldType::Bool => raw
            .first()
            .and_then(|r| r.parse::<bool>().ok())
            .map(Value::from)
            .ok_or(format!("`{name}` takes true or false")),
        FieldType::U64 => raw
            .first()
            .and_then(|r| r.parse::<u64>().ok())
            .map(Value::from)
            .ok_or(format!("`{name}` takes a whole number")),
        FieldType::StrList => list(raw, name),
        FieldType::Object => serde_json::from_str::<Value>(&raw.join(" "))
            .ok()
            .filter(Value::is_object)
            .ok_or(format!("`{name}` takes a JSON object")),
    }
}

fn list(raw: &[String], name: &str) -> Result<Value, String> {
    if let [text] = raw
        && text.trim_start().starts_with('[')
    {
        return serde_json::from_str::<Vec<String>>(text)
            .map(|items| json!(items))
            .map_err(|e| format!("`{name}` takes a JSON array of strings: {e}"));
    }
    // `carries` keeps a blank field so `start` refuses `hf-551,` as the reference says (fac-606).
    let keep_blank = name == "carries";
    Ok(json!(raw.iter().flat_map(|r| r.split(',')).filter(|s| keep_blank || !s.is_empty()).collect::<Vec<_>>()))
}

/// Fields that are free text: the last required one of a command takes the rest of its words, and
/// so does its one optional text field.
pub const FREE_TEXT: [&str; 8] = ["text", "why", "note", "question", "title", "description", "task", "words"];

fn named_fields(spec: &CommandSpec, args: &[String], body: &mut Map<String, Value>) -> Result<Vec<String>, String> {
    let mut positional = Vec::new();
    let mut files = Vec::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if let Some(name) = a.strip_prefix("--") {
            let f =
                spec.fields.iter().find(|f| f.name == name).ok_or(format!("`{}` has no field `{name}`", spec.name))?;
            if f.ty == FieldType::Bool {
                body.insert(name.to_owned(), json!(true));
                i += 1;
                continue;
            }
            let v = args.get(i + 1).ok_or(format!("--{name} needs a value"))?;
            if spec.name == "notes.add" && name == "file" {
                files.push(v);
            } else {
                body.insert(name.to_owned(), value(f.ty, std::slice::from_ref(v), name)?);
            }
            i += 2;
        } else {
            positional.push(a.clone());
            i += 1;
        }
    }
    if !files.is_empty() {
        body.insert("file".to_owned(), json!(files));
    }
    Ok(positional)
}

// `start` carries the words after its card unless the first of them is a mode word (fac-563): `start A B C`, but
// `start A report opus high` as before. Each such word is one card, read as typed: `[]` or `,` is no list.
fn carried(spec: &CommandSpec, positional: &mut Vec<String>, body: &mut Map<String, Value>) -> Result<(), String> {
    // An empty word (an unset `"$member"`) names no card: refused, never read as one card fewer.
    let empty = || Err("`start` carries no empty card: name each card it carries".to_owned());
    if spec.name != "start" {
        return Ok(());
    }
    if let Some(named) = body.get("carries") {
        let items = named.as_array().map(Vec::as_slice).unwrap_or_default();
        let blank = |v: &Value| v.as_str().is_none_or(|w| w.trim().is_empty());
        return if items.is_empty() || items.iter().any(blank) { empty() } else { Ok(()) };
    }
    let first = usize::from(!body.contains_key("card"));
    let mode_word = |w: &String| serde_json::from_value::<crate::card::Mode>(json!(w)).is_ok();
    if positional.get(first).is_some_and(|w| !mode_word(w)) {
        let words = positional.split_off(first);
        if words.iter().any(|w| w.trim().is_empty()) {
            return empty();
        }
        body.insert("carries".to_owned(), json!(words));
    }
    Ok(())
}

fn positional_option(command: &str, field: &Field) -> bool {
    match (command, field.name) {
        ("notes.add", "title") => false,
        ("notes.add", "file") => true,
        _ => matches!(field.ty, FieldType::Str | FieldType::U64),
    }
}

/// Builds command JSON from words: named fields, then required positional fields in order.
/// The last free-text or list field takes the remaining words; boolean flags need no value.
pub fn command(spec: &CommandSpec, args: &[String]) -> Result<Value, String> {
    let mut body = Map::new();
    let mut positional = named_fields(spec, args, &mut body)?;
    carried(spec, &mut positional, &mut body)?;
    // A required field may be named too (`door --target t --op o --params '{}'`).
    let required: Vec<_> = spec.fields.iter().filter(|f| !f.optional && !body.contains_key(f.name)).collect();
    let takes_rest = required.last().is_some_and(|f| FREE_TEXT.contains(&f.name) || f.ty == FieldType::StrList);
    if !takes_rest && positional.len() > required.len() {
        let extra = positional.split_off(required.len());
        // A word fills an optional one too.
        let open: Vec<_> = spec
            .fields
            .iter()
            .filter(|f| f.optional && positional_option(spec.name, f) && !body.contains_key(f.name))
            .collect();
        match open.as_slice() {
            [only] if FREE_TEXT.contains(&only.name) => {
                body.insert(only.name.to_owned(), json!(extra.join(" ")));
            }
            _ => {
                let mut extra = extra.into_iter();
                for (f, v) in open.into_iter().zip(extra.by_ref()) {
                    body.insert(f.name.to_owned(), value(f.ty, std::slice::from_ref(&v), f.name)?);
                }
                positional.extend(extra);
            }
        }
    }
    for (n, f) in required.iter().enumerate() {
        let last = n + 1 == required.len();
        let raw: Vec<String> = if last && f.ty != FieldType::U64 && !positional.is_empty() {
            std::mem::take(&mut positional)
        } else if last && f.ty == FieldType::StrList {
            Vec::new()
        } else if positional.is_empty() {
            return Err(format!("`{}` needs `{}`", spec.name, f.name));
        } else {
            vec![positional.remove(0)]
        };
        let v = if (spec.name, f.name) == ("deliverables", "names") {
            json!(raw.join("\n"))
        } else {
            value(f.ty, &raw, f.name)?
        };
        body.insert(f.name.to_owned(), v);
    }
    if !positional.is_empty() {
        return Err(format!("`{}` takes no more arguments: {}", spec.name, positional.join(" ")));
    }
    if spec.name == "notes.add"
        && body["kind"] != "proposal"
        && let Some(Value::Array(files)) = body.get("file")
    {
        let [only] = files.as_slice() else { return Err("only a proposal accepts more than one --file".into()) };
        body.insert("file".to_owned(), only.clone());
    }
    Ok(json!({ spec.name: body }))
}
