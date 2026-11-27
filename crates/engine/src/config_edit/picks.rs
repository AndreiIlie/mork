use mork_types::config::PickModel;
use toml_edit::{Array, InlineTable, Value};

use super::document::{self, Result};

fn same(a: &PickModel, b: &PickModel) -> bool {
    a.cli == b.cli && a.model == b.model
}

fn same_entry(a: &PickModel, b: &PickModel) -> bool {
    same(a, b) && a.label == b.label
}

fn efforts(p: &PickModel) -> Value {
    Value::Array(p.efforts.iter().map(|e| e.as_str()).collect())
}

fn table(p: &PickModel) -> InlineTable {
    let mut t = InlineTable::new();
    t.insert("cli", p.cli.as_str().into());
    t.insert("model", p.model.as_str().into());
    t.insert("label", p.label.as_str().into());
    t.insert("efforts", efforts(p));
    t
}

fn validate(old: &[PickModel], next: &[PickModel], offers: &[PickModel]) -> Result<()> {
    for (i, pick) in next.iter().enumerate() {
        let existing = old.iter().find(|p| same_entry(p, pick));
        let choice = offers.iter().find(|p| same(p, pick));
        let preceding = next[..i].iter().filter(|p| same(p, pick)).count();
        if preceding >= old.iter().filter(|p| same(p, pick)).count().max(1) {
            return Err("This model is listed twice.".into());
        }
        if existing == Some(pick) {
            continue;
        }
        let label = existing
            .or_else(|| old.iter().find(|p| same(p, pick)))
            .or(choice)
            .ok_or("This model is unavailable here.")?;
        if label.label != pick.label {
            return Err("Use the configured model label.".into());
        }
        if pick
            .efforts
            .iter()
            .any(|e| !choice.is_some_and(|p| p.efforts.contains(e)) && !existing.is_some_and(|p| p.efforts.contains(e)))
        {
            return Err("This effort is unavailable for that model.".into());
        }
    }
    Ok(())
}

pub(super) fn write(
    s: &mut String,
    key: &str,
    old: &[PickModel],
    next: &[PickModel],
    offers: &[PickModel],
) -> Result<()> {
    validate(old, next, offers)?;
    let path: Vec<_> = std::iter::once("picks").chain(key.split('.')).collect();
    let parsed = document::doc(s)?;
    let array = document::item(parsed.as_item(), &path).and_then(|v| v.as_array_of_tables());
    if let Some(array) = array {
        let mut rows = Vec::new();
        let mut remaining = next;
        for (p, t) in old.iter().zip(array.iter()) {
            let new = remaining.first().filter(|n| same_entry(n, p));
            if new.is_some() {
                remaining = &remaining[1..];
            }
            if new != Some(p) {
                rows.push((t.clone(), new));
            }
        }
        for (t, p) in rows.into_iter().rev() {
            if let Some(p) = p {
                let span = t.get("efforts").and_then(|v| v.span()).ok_or("Missing model efforts.")?;
                let replacement = super::arrays::formatted(&s[span.clone()], &efforts(p));
                s.replace_range(span, &replacement);
            } else {
                document::remove_pick(s, &t)?;
            }
        }
        for p in remaining {
            let t = table(p);
            s.push_str(&format!("\n[[{}]]\n", path.join(".")));
            for (k, v) in t.iter() {
                s.push_str(&format!("{k} = {v}\n"));
            }
        }
    } else {
        let list: Array = next.iter().map(|p| Value::InlineTable(table(p))).collect();
        document::set(s, &path, (!next.is_empty()).then_some(Value::Array(list)))?;
    }
    if next.is_empty() {
        if key.starts_with("reviewer.") {
            document::prune(s, &["picks", "reviewer"])?;
        }
        document::prune(s, &["picks"])?;
    }
    Ok(())
}
