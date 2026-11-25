// Mongo filters retain the server's query language, except expressions that execute supplied
// code. Literal documents remain data; projection, sort and request bounds stay fixed (fac-124).

use serde_json::{Map, Value};

const DEPTH: usize = 16;
const BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Query,
    Predicate,
    Expression,
    Literal,
    All,
}

/// A collection passed literally to the client; the server decides which namespaces it reads.
pub(super) fn collection(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('\0') {
        Err("a collection's name is nonempty and contains no NUL".into())
    } else {
        Ok(())
    }
}

fn field(name: &str) -> Result<(), String> {
    if name.is_empty() || name.starts_with('$') || name.contains('\0') || name.len() > 256 {
        return Err(format!("`{name}` isn't a field the door reads"));
    }
    Ok(())
}

/// A find filter without executable code, at most 16 deep and 16 KiB.
pub(super) fn filter(f: &Map<String, Value>) -> Result<(), String> {
    if Value::Object(f.clone()).to_string().len() > BYTES {
        return Err("the filter is over 16 KiB".into());
    }
    inspect(&Value::Object(f.clone()), Context::Query, 0)
}

fn inspect(v: &Value, mut context: Context, depth: usize) -> Result<(), String> {
    match v {
        Value::Object(o) => {
            if depth > DEPTH {
                return Err("the filter nests too deep".into());
            }
            // An embedded equality document is literal, even when its own values contain `$` keys.
            if context == Context::Predicate && literal(o) {
                context = Context::Literal;
            }
            for (k, v) in o {
                inspect(v, child(context, k)?, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                let child = match context {
                    Context::Predicate => Context::Literal,
                    Context::All if v.get("$elemMatch").is_none() => Context::Literal,
                    Context::Query | Context::Expression | Context::Literal | Context::All => context,
                };
                inspect(v, child, depth + 1)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn literal(o: &Map<String, Value>) -> bool {
    let code =
        o.get("$code").is_some_and(Value::is_string) && o.keys().all(|k| matches!(k.as_str(), "$code" | "$scope"));
    let reference = o.get("$ref").is_some_and(Value::is_string)
        && o.contains_key("$id")
        && o.keys().all(|k| matches!(k.as_str(), "$ref" | "$id" | "$db"));
    // Extended JSON code values and DBRefs are compared as BSON data, never invoked by find.
    o.keys().all(|k| !k.starts_with('$')) || code || reference
}

fn child(context: Context, key: &str) -> Result<Context, String> {
    use Context::{All, Expression, Literal, Predicate, Query};
    if context == Literal || (context == All && key != "$elemMatch") {
        return Ok(Literal);
    }
    if matches!(key, "$where" | "$function" | "$accumulator" | "$_internalJsEmit" | "$_internalJsReduce") {
        return Err(format!("`{key}` executes code: the door only reads"));
    }
    if context == Expression {
        return Ok(if matches!(key, "$literal" | "$const") { Literal } else { Expression });
    }
    Ok(match key {
        "$expr" => Expression,
        "$and" | "$or" | "$nor" | "$elemMatch" | "$query" => Query,
        "$not" => Predicate,
        "$all" => All,
        "$eq" | "$ne" | "$gt" | "$gte" | "$lt" | "$lte" | "$in" | "$nin" | "$jsonSchema" | "$comment" | "$text" => {
            Literal
        }
        _ if !key.starts_with('$') => Predicate,
        _ => Query,
    })
}

/// The projection: fields to include, each `1` or `true`; their names, in order.
pub(super) fn projection(p: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (k, v) in p {
        field(k)?;
        let include = v.as_u64() == Some(1) || v.as_bool() == Some(true);
        if !include {
            return Err(format!("the projection's `{k}` must be 1: the door's projections include fields"));
        }
        out.push(k.clone());
    }
    Ok(out)
}

/// The sort: fields, each 1 or -1.
pub(super) fn sort(s: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in s {
        field(k)?;
        if !matches!(v.as_i64(), Some(1 | -1)) {
            return Err(format!("the sort's `{k}` must be 1 or -1"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn code_is_refused_but_server_operators_and_literals_are_read() {
        for f in [
            json!({"$where": "this.a == 1"}),
            json!({"a": {"$function": {"body": "return 1", "args": [], "lang": "js"}}}),
            json!({"$expr": {"$gt": [1, {"$function": {"body": "return 1", "args": [], "lang": "js"}}]}}),
            json!({"$expr": {"$_internalJsEmit": {"eval": "function(){}", "this": {}}}}),
            json!({"$expr": {"$_internalJsReduce": {"eval": "function(){}", "data": []}}}),
            json!({"a": {"$all": [{"$elemMatch": {"$where": "1"}}]}}),
            json!({"a": {"$accumulator": {}}}),
            json!({"$or": [{"a": 1}, {"$where": "1"}]}),
            json!({"a": {"$elemMatch": {"$where": "1"}}}),
        ] {
            assert!(filter(&obj(f.clone())).is_err(), "read: {f}");
        }
        for f in [
            json!({"a": 1, "b": {"$gte": 2, "$lt": 9}}),
            json!({"$expr": {"$gt": ["$a", 1]}, "flags": {"$bitsAllSet": 3}}),
            json!({"$expr": {"$eq": ["$a", {"$literal": {"$function": "data"}}]}}),
            json!({"$expr": {"$const": {"$where": "data"}}}),
            json!({"a": {"$eq": {"$where": "data"}}, "b": {"$in": [{"$function": "data"}]}}),
            json!({"a": {"nested": {"$accumulator": "data"}}, "$comment": {"$where": "data"}}),
            json!({"a": [{"$where": "data"}, {"nested": {"$function": "data"}}]}),
            json!({"a": {"$code": "data", "$scope": {"$where": "data"}}}),
            json!({"a": {"$ref": "other", "$id": {"$function": "data"}}}),
            json!({"a": {"$all": [[{"$elemMatch": {"$where": "data"}}]]}}),
            json!({"a": {"$all": [{"$function": "data"}, {"$elemMatch": {"b": {"$size": 2}}}]}}),
            json!({"$or": [{"a": {"$in": [1, 2]}}, {"b": {"$exists": false}}]}),
            json!({"_id": {"$oid": "5f1d7f1b2c1e4a0012345678"}, "at": {"$gt": {"$date": "2026-01-01T00:00:00Z"}}}),
        ] {
            assert!(filter(&obj(f.clone())).is_ok(), "refused: {f}");
        }
        let mut nested = json!({"leaf": 1});
        for _ in 0..DEPTH {
            nested = json!({"a": nested});
        }
        assert!(filter(&obj(nested.clone())).is_ok(), "the existing depth bound includes scalar leaves");
        assert!(filter(&obj(json!({"a": nested}))).is_err());
        assert!(projection(&obj(json!({"a": 0}))).is_err());
        assert!(projection(&obj(json!({"a": {"$slice": 1}}))).is_err());
        assert!(sort(&obj(json!({"a": 2}))).is_err());
        assert!(collection("system.users").is_ok());
        assert!(collection("a$b and Unicode α").is_ok());
        assert!(collection("bad\0name").is_err());
    }
}
