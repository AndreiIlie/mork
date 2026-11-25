//! Door answers keep decoded values masked and byte cuts valid, with whole rows or documents
//! retained where possible (fac-1).

use factory_types::door::Output;
use factory_types::door_plan::{Bounds, Plan, Ran};
use serde_json::{Value, json};

use super::redact::Redactor;

/// The answer's body for what ran: `status` done, with rows, json or text; or failed, with the
/// target's own words. A database's answer has its server's own time, null when it has none.
pub fn shape(plan: &Plan, ran: &Ran, b: &Bounds, r: &Redactor) -> Value {
    let mut v = body(plan, ran, b, r);
    if matches!(plan, Plan::Sql { .. } | Plan::Mongo { .. }) {
        v["server_us"] = json!(ran.server_us);
    }
    if matches!(plan, Plan::Sql { .. }) && ran.cut_at_cap == Some(true) && ran.server_us.is_none() {
        v["server_us_why"] =
            json!("the answer was cut at the target's size cap before the query's time could be read from its server");
    }
    // A copied file's answer is where it went and its size, never its bytes.
    if matches!(plan, Plan::Get { .. })
        && v["status"] == "done"
        && let Ok(got) = serde_json::from_str::<Value>(&ran.stdout)
    {
        v["file"] = got["file"].clone();
        v["size"] = got["size"].clone();
    }
    v
}

fn body(plan: &Plan, ran: &Ran, b: &Bounds, r: &Redactor) -> Value {
    let max = usize::try_from(b.max_bytes).unwrap_or(usize::MAX);
    let items = usize::try_from(b.max_items).unwrap_or(usize::MAX);
    // A command cut at its bound was ended there (or died on the closed pipe): what it gave
    // stands, marked `truncated`.
    if ran.timed_out || (ran.exit != Some(0) && !ran.truncated) {
        let words = if ran.stderr.trim().is_empty() { &ran.stdout } else { &ran.stderr };
        let mut out = Output {
            text: Some(r.mask_text(words, true)),
            truncated: ran.truncated,
            elapsed: ran.elapsed,
            ..Output::default()
        };
        fit(&mut out, max);
        let mut v = serde_json::to_value(out).unwrap_or(Value::Null);
        v["status"] = json!(if ran.timed_out { "failed: timed out" } else { "failed" });
        v["exit"] = json!(ran.exit);
        return v;
    }
    let mut out = match plan {
        Plan::Sql { .. } => rows(&ran.stdout, ran.truncated, items, r),
        Plan::Mongo { .. } => docs(&r.mask_json(&ran.stdout), ran.truncated, items),
        Plan::Libra { .. } => {
            let masked = r.mask_json(&ran.stdout);
            match serde_json::from_str::<Value>(&masked) {
                Ok(v) if !ran.truncated => Output { json: Some(v), ..Output::default() },
                _ => Output { text: Some(masked), truncated: ran.truncated, ..Output::default() },
            }
        }
        Plan::Get { .. } => Output::default(),
        Plan::Read { .. } | Plan::Tail { .. } | Plan::Run { .. } => text(&ran.stdout, ran.truncated, r),
    };
    out.elapsed = ran.elapsed;
    fit(&mut out, max);
    let mut v = serde_json::to_value(out).unwrap_or(Value::Null);
    v["status"] = json!("done");
    v
}

fn text(t: &str, cut: bool, r: &Redactor) -> Output {
    let text = r.mask_text(t, cut);
    Output { text: Some(text), truncated: cut, ..Output::default() }
}

fn unescape(f: &str) -> String {
    let mut out = String::with_capacity(f.len());
    let mut chars = f.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

// `NULL` reads as null; a cut output loses its last line, which may be partial.
fn rows(t: &str, cut: bool, items: usize, r: &Redactor) -> Output {
    let mut lines: Vec<&str> = t.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut truncated = false;
    if cut {
        lines.pop();
        truncated = true;
    }
    let field = |f: &str| if f == "NULL" { Value::Null } else { Value::String(r.mask_text(&unescape(f), false)) };
    let columns =
        lines.first().map(|l| l.split('\t').map(|f| r.mask_text(&unescape(f), false)).collect()).unwrap_or_default();
    let mut rows: Vec<Vec<Value>> = lines.iter().skip(1).map(|l| l.split('\t').map(field).collect()).collect();
    if rows.len() > items {
        rows.truncate(items);
        truncated = true;
    }
    Output { columns: Some(columns), rows: Some(rows), truncated, ..Output::default() }
}

// `t` is masked already; a cut output loses its last line.
fn docs(t: &str, cut: bool, items: usize) -> Output {
    let mut lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut truncated = false;
    if cut {
        lines.pop();
        truncated = true;
    }
    let mut docs: Vec<Value> = lines
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap_or_else(|_| Value::String((*l).to_owned())))
        .collect();
    if docs.len() > items {
        docs.truncate(items);
        truncated = true;
    }
    Output { json: Some(Value::Array(docs)), truncated, ..Output::default() }
}

fn size<T: serde::Serialize + ?Sized>(o: &T) -> usize {
    serde_json::to_string(o).map_or(0, |s| s.len())
}

fn fit_items<T: serde::Serialize>(items: &mut Vec<T>, bytes: &mut usize, max: usize) {
    while *bytes > max {
        let Some(item) = items.pop() else { break };
        *bytes = bytes.saturating_sub(size(&item) + usize::from(!items.is_empty()));
    }
}

/// Cuts an output to `max` bytes: whole rows, then whole columns from the right (each row's
/// cell with its column), whole documents, or text at a character.
pub fn fit(o: &mut Output, max: usize) {
    let mut bytes = size(o);
    if bytes <= max {
        return;
    }
    // Removing each encoded item once keeps unlimited replica rows from making fitting quadratic.
    bytes -= usize::from(!o.truncated);
    o.truncated = true;
    if let Some(rows) = &mut o.rows {
        fit_items(rows, &mut bytes, max);
    }
    if bytes > max
        && let Some(cols) = &mut o.columns
    {
        fit_items(cols, &mut bytes, max);
        for row in o.rows.iter_mut().flatten() {
            row.truncate(cols.len());
        }
    }
    if let Some(Value::Array(docs)) = &mut o.json {
        fit_items(docs, &mut bytes, max);
    }
    if bytes > max
        && let Some(j) = o.json.take()
    {
        o.text = Some(j.to_string());
        bytes = size(o);
    }
    if let Some(t) = &mut o.text {
        while bytes > max {
            let Some(c) = t.pop() else { break };
            bytes = bytes.saturating_sub(size(&c) - 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql() -> Plan {
        Plan::Sql { login: login(), host: "h".into(), port: 1, database: "d".into(), query: "q".into() }
    }

    fn login() -> factory_types::door_config::Login {
        factory_types::door_config::Login { section: "s".into(), user: "user".into(), password: "password".into() }
    }

    // Second review, finding 1: secrets spelled as JSON escapes must also be masked.
    #[test]
    fn fac1_failure_words_hide_escaped_secrets() {
        let mut r = Redactor::default();
        r.add("SENTINEL-hostpw-77");
        let b = Bounds { max_bytes: 1024, max_items: 100, timeout_ms: 1 };
        let failed = Ran {
            exit: Some(1),
            stderr: r#"{"error":"denied","pw":"\u0053ENTINEL-hostpw-77"}"#.into(),
            ..Ran::default()
        };
        let v = shape(&sql(), &failed, &b, &r);
        let text = v["text"].as_str().unwrap();
        assert_eq!(v["status"], json!("failed"));
        assert!(text.contains("denied") && !crate::door::redact::unescape_json(text).contains("SENTINEL"), "{text}");
    }
}
