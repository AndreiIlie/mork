use toml_edit::{Array, Decor, RawString, Value};

fn keep_comments(raw: Option<&RawString>, out: &mut String) {
    for line in raw.and_then(RawString::as_str).unwrap_or_default().lines() {
        if let Some(at) = line.find('#') {
            out.push_str(&line[at..]);
            out.push('\n');
        }
    }
}

fn decorations(d: &Decor, out: &mut String) {
    keep_comments(d.prefix(), out);
    keep_comments(d.suffix(), out);
}

// Only decorations contain TOML comments. A hash inside a string, including a checks command, belongs to its value.
pub(super) fn comments(value: &Value, out: &mut String) {
    decorations(value.decor(), out);
    if let Some(a) = value.as_array() {
        for v in a {
            comments(v, out);
        }
        keep_comments(Some(a.trailing()), out);
    }
    if let Some(t) = value.as_inline_table() {
        for (key, v) in t {
            if let Some(k) = t.key(key) {
                decorations(k.leaf_decor(), out);
            }
            comments(v, out);
        }
        keep_comments(Some(t.trailing()), out);
    }
}

fn same(old: &Value, new: &Value) -> bool {
    match (old, new) {
        (Value::String(a), Value::String(b)) => a.value() == b.value(),
        (Value::Integer(a), Value::Integer(b)) => a.value() == b.value(),
        (Value::InlineTable(a), Value::InlineTable(b)) => ["model", "cli", "label"]
            .iter()
            .all(|key| a.get(key).and_then(Value::as_str) == b.get(key).and_then(Value::as_str)),
        _ => false,
    }
}

fn retained(mut value: Value, next: &Value) -> Value {
    if let Some(t) = value.as_inline_table_mut()
        && let Some(old) = t.get_mut("efforts").and_then(Value::as_array_mut)
        && let Some(new) = next.as_inline_table().and_then(|v| v.get("efforts")).and_then(Value::as_array)
    {
        *old = edit(old, new);
    }
    value
}

fn edit(old: &Array, next: &Array) -> Array {
    let mut pool: Vec<_> = old.iter().cloned().collect();
    let mut result = old.clone();
    result.clear();
    for entry in next {
        let v = pool
            .iter()
            .position(|old| same(old, entry))
            .map(|i| retained(pool.remove(i), entry))
            .unwrap_or_else(|| entry.clone());
        result.push_formatted(v);
    }
    let mut notes = String::new();
    for removed in pool {
        comments(&removed, &mut notes);
    }
    if !notes.is_empty() {
        result.set_trailing(format!("\n{notes}{}", old.trailing().as_str().unwrap_or_default()));
    }
    if result.is_empty() {
        result.set_trailing_comma(false);
    }
    result
}

pub(super) fn formatted(previous: &str, next: &Value) -> String {
    if let Some(new) = next.as_array()
        && let Ok(Value::Array(old)) = previous.parse::<Value>()
    {
        return edit(&old, new).to_string();
    }
    next.to_string()
}
