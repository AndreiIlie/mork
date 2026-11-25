//! The door's SQL read gate (fac-124): one statement that reads, as MySQL writes it, with no table,
//! column or syntax list. The server's read-only session is what refuses a write; this names one first.

use std::ops::Range;

use super::lex::{Tok, Token, fragment, tokens};

// The words a read begins with; a leading `(` is a SELECT's too.
const LEADS: &[&str] = &["SELECT", "WITH", "TABLE", "VALUES", "SHOW", "DESCRIBE", "DESC", "EXPLAIN"];

/// Why a read of `information_schema` or `sys` is refused (fac-183, fac-721): scanning them opens
/// every database's tables and can overload or crash the server.
pub(super) const SCHEMA_REFUSED: &str = "refused: reading information_schema or the sys schema is not allowed because it scans metadata across every database on the server.";

// Reserved words can be identifiers after a period or function names before `(`.
const WRITES: &[&str] = &["INSERT", "UPDATE", "DELETE", "REPLACE"];

/// A query past the read gate, parsed once: its tokens and every view its executable comments give.
pub struct Read<'q> {
    query: &'q str,
    toks: Vec<Token>,
    views: Vec<Vec<Token>>,
}

/// Reads `query`, or says why it isn't a read: one statement that leads as a read, with no write,
/// INTO, lock (but under a plain EXPLAIN) or (inside an executable comment) hidden statement of any of these.
pub fn read(query: &str) -> Result<Read<'_>, String> {
    let toks = tokens(query)?;
    let views: Vec<Vec<Token>> = Scan::new(query, &toks)?.views().collect();
    for parts in &views {
        let Some(first) = parts.first() else { continue };
        let leads = first.tok == Tok::Sym("(")
            || matches!(&first.tok, Tok::Word(w) if LEADS.iter().any(|l| w.eq_ignore_ascii_case(l)));
        if !leads {
            return Err(format!(
                "`{}` isn't a read: a query begins with {} or `(`",
                &query[first.at.clone()],
                LEADS.join(", ")
            ));
        }
        let plans = planned(parts);
        for (n, t) in parts.iter().enumerate() {
            refused(t, parts[..n].last(), parts.get(n + 1), plans)?;
            schema(parts, n)?;
            match &t.tok {
                Tok::Hint(body) => hint(body, query[..t.at.start].chars().count())?,
                Tok::Sym(";") => return Err("more than one statement: one read at a time".into()),
                Tok::Word(_) | Tok::Quoted(_) | Tok::Num(_) | Tok::Str(_) | Tok::Exec(_) | Tok::Sym(_) => {}
            }
        }
    }
    if views.iter().all(Vec::is_empty) {
        return Err("the query is empty".into());
    }
    Ok(Read { query, toks, views })
}

impl Read<'_> {
    /// The one statement that runs, comments dropped and whitespace between tokens one space (a
    /// string's own bytes kept). It reads as itself.
    pub fn text(&self) -> String {
        render(self.query, &self.toks, &[])
    }

    /// As `text`, lowering an outer LIMIT above `cap + 1` while retaining its offset.
    /// The extra row lets the caller report truncation; inner subqueries keep their own limits.
    pub fn capped(&self, cap: u64) -> Result<String, String> {
        let mut subs = std::collections::BTreeMap::new();
        let mut offsets = Vec::new();
        let mut wrap = false;
        let mut inner = Vec::new();
        for parts in &self.views {
            let result = limit(parts).map(|(count, _)| count.at.start);
            for (n, t) in parts.iter().enumerate() {
                if matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("LIMIT"))
                    && let Some((count, _)) = arguments(&parts[n + 1..])
                    && Some(count.at.start) != result
                {
                    inner.push(count.at.start);
                }
            }
            wrap |= limit(parts).is_none() && outer(parts).first().is_some_and(|t| t.tok == Tok::Sym("("));
            if let Some((count, offset)) = limit(parts) {
                offsets.extend(offset.map(|t| t.at.start));
                if let Tok::Num(s) = &count.tok
                    && s.bytes().all(|b| b.is_ascii_digit())
                    && s.parse::<u64>().unwrap_or(u64::MAX) > cap.saturating_add(1)
                {
                    subs.insert(count.at.start, (count.at.clone(), cap.saturating_add(1).to_string()));
                }
            }
        }
        if offsets.iter().chain(&inner).any(|at| subs.contains_key(at)) {
            return Err(
                "a version-gated LIMIT changes its count into an offset or inner limit; keep its clause together"
                    .into(),
            );
        }
        let sql = render(self.query, &self.toks, &subs.into_values().collect::<Vec<_>>());
        // A parenthesized result with outer ordering or set operations may carry an inner LIMIT that
        // overrides the session default. An expression wrapper caps its result without renaming columns.
        Ok(if wrap && cap != u64::MAX { format!("({sql}) LIMIT {}", cap.saturating_add(1)) } else { sql })
    }

    /// Whether the query's hints keep its server's time limit, the call's `timeout_ms` (fac-183).
    pub fn timed(&self, timeout_ms: u64) -> Result<(), String> {
        for t in self.views.iter().flatten() {
            if let Tok::Hint(body) = &t.tok {
                hint_time(body, self.query[..t.at.start].chars().count(), timeout_ms)?;
            }
        }
        Ok(())
    }
}

// Original spans keep executable guards intact while a replacement can address their body.
fn render(query: &str, toks: &[Token], subs: &[(Range<usize>, String)]) -> String {
    toks.iter()
        .enumerate()
        .map(|(n, t)| {
            let mut text = query[t.at.clone()].to_owned();
            for (at, val) in subs.iter().rev().filter(|(at, _)| t.at.contains(&at.start)) {
                text.replace_range(at.start - t.at.start..at.end - t.at.start, val);
            }
            if n > 0 && t.spaced { format!(" {text}") } else { text }
        })
        .collect()
}

// CTE bodies must keep their limits: only the query expression after their definitions is capped.
fn after_ctes(toks: &[Token]) -> Option<&[Token]> {
    let mut rest = toks;
    loop {
        let at = rest.iter().position(|t| matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("AS")))?;
        rest = &rest[at + 1..];
        let end = closing(rest)?;
        rest = &rest[end + 1..];
        if !rest.first().is_some_and(|t| t.tok == Tok::Sym(",")) {
            return Some(rest);
        }
        rest = &rest[1..];
    }
}

fn closing(toks: &[Token]) -> Option<usize> {
    if !toks.first().is_some_and(|t| t.tok == Tok::Sym("(")) {
        return None;
    }
    let mut depth = 0;
    toks.iter().position(|t| {
        match t.tok {
            Tok::Sym("(") => depth += 1,
            Tok::Sym(")") => depth -= 1,
            Tok::Word(_) | Tok::Quoted(_) | Tok::Num(_) | Tok::Str(_) | Tok::Hint(_) | Tok::Exec(_) | Tok::Sym(_) => {}
        }
        depth == 0
    })
}

// Parentheses enclosing the result are wrappers, unlike parentheses in a SELECT's expressions.
fn outer(toks: &[Token]) -> &[Token] {
    let mut parts = toks;
    loop {
        if parts.first().is_some_and(|t| matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("WITH"))) {
            parts = after_ctes(&parts[1..]).unwrap_or(parts);
        }
        if let Some(end) = closing(parts)
            && end + 1 == parts.len()
        {
            parts = &parts[1..end];
        } else {
            return parts;
        }
    }
}

fn limit(toks: &[Token]) -> Option<(&Token, Option<&Token>)> {
    let toks = outer(toks);
    let mut depth = 0i32;
    let mut found = None;
    for (n, t) in toks.iter().enumerate() {
        match &t.tok {
            Tok::Sym("(") => depth += 1,
            Tok::Sym(")") => depth -= 1,
            Tok::Word(w) if depth == 0 && w.eq_ignore_ascii_case("LIMIT") => found = Some(n),
            Tok::Word(_) | Tok::Quoted(_) | Tok::Num(_) | Tok::Str(_) | Tok::Hint(_) | Tok::Exec(_) | Tok::Sym(_) => {}
        }
    }
    arguments(&toks[found? + 1..])
}

fn arguments(toks: &[Token]) -> Option<(&Token, Option<&Token>)> {
    if toks.get(1).is_some_and(|t| t.tok == Tok::Sym(",")) {
        Some((toks.get(2)?, toks.first()))
    } else {
        let offset = toks.get(1).is_some_and(|t| matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("OFFSET")));
        Some((toks.first()?, if offset { toks.get(2) } else { None }))
    }
}

// A plain EXPLAIN only plans its statement (fac-255); ANALYZE anywhere in the view runs it.
fn planned(parts: &[Token]) -> bool {
    let word = |t: &Token, k: &str| matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case(k));
    parts.first().is_some_and(|t| word(t, "EXPLAIN"))
        && !parts
            .iter()
            .enumerate()
            .any(|(n, t)| word(t, "ANALYZE") && !n.checked_sub(1).is_some_and(|p| parts[p].tok == Tok::Sym(".")))
}

fn refused(t: &Token, previous: Option<&Token>, next: Option<&Token>, plans: bool) -> Result<(), String> {
    let Tok::Word(w) = &t.tok else { return Ok(()) };
    if previous.is_some_and(|p| p.tok == Tok::Sym(".")) {
        return Ok(());
    }
    let up = w.to_ascii_uppercase();
    let next_is = |k: &str| next.is_some_and(|n| matches!(&n.tok, Tok::Word(x) if x.eq_ignore_ascii_case(k)));
    let after_for = previous.is_some_and(|p| matches!(&p.tok, Tok::Word(x) if x.eq_ignore_ascii_case("FOR")));
    if plans && after_for && up == "UPDATE" {
        return Ok(());
    }
    if WRITES.contains(&up.as_str()) && !next.is_some_and(|n| n.tok == Tok::Sym("(")) {
        return Err(format!("`{up}` writes: the door only reads"));
    }
    if up == "INTO" {
        return Err("`INTO` writes a file or a variable: the door only reads".into());
    }
    if !plans && (up == "LOCK" || (up == "FOR" && (next_is("UPDATE") || next_is("SHARE")))) {
        return Err("a locking read isn't read here: it holds rows others write".into());
    }
    Ok(())
}

// `information_schema` anywhere, or `sys` where it names a schema: before a period, or after FROM/IN
// of a SHOW (its `sys` views scan information_schema). A column or alias called `sys` passes.
fn schema(parts: &[Token], n: usize) -> Result<(), String> {
    let (Tok::Word(w) | Tok::Quoted(w)) = &parts[n].tok else { return Ok(()) };
    let word =
        |t: Option<&Token>, k: &str| t.is_some_and(|t| matches!(&t.tok, Tok::Word(x) if x.eq_ignore_ascii_case(k)));
    let previous = n.checked_sub(1).map(|p| &parts[p]);
    let named = previous.is_none_or(|p| p.tok != Tok::Sym(".") && !word(Some(p), "AS"));
    let qualifier = parts.get(n + 1).is_some_and(|t| t.tok == Tok::Sym("."));
    let shown = word(parts.first(), "SHOW") && (word(previous, "FROM") || word(previous, "IN"));
    if w.eq_ignore_ascii_case("information_schema") || (w.eq_ignore_ascii_case("sys") && named && (qualifier || shown))
    {
        return Err(SCHEMA_REFUSED.into());
    }
    Ok(())
}

// Executable guards must not hide a write or a LIMIT from the read gate.
struct Scan {
    parts: Vec<(Token, Option<(bool, u64)>)>,
    versions: std::collections::BTreeSet<u64>,
}

impl Scan {
    fn new(query: &str, toks: &[Token]) -> Result<Self, String> {
        let mut scan = Self { parts: Vec::new(), versions: [0].into() };
        for t in toks {
            let Tok::Exec(whole) = &t.tok else {
                scan.parts.push((t.clone(), None));
                continue;
            };
            let maria = whole.starts_with("/*M!");
            let start = if maria { 4 } else { 3 };
            let body = &whole[start..whole.len() - 2];
            let skip = body.bytes().take_while(u8::is_ascii_digit).count();
            let version = body[..skip].parse::<u64>().unwrap_or(0);
            scan.versions.insert(version);
            let offset = query[..t.at.start + start + skip].chars().count();
            for mut inner in fragment(&body[skip..], offset)? {
                if matches!(inner.tok, Tok::Exec(_) | Tok::Sym(";")) {
                    return Err("a statement delimiter or nested executable comment isn't read".into());
                }
                inner.at = (inner.at.start + t.at.start + start + skip)..(inner.at.end + t.at.start + start + skip);
                scan.parts.push((inner, Some((maria, version))));
            }
        }
        Ok(scan)
    }

    // Guards change only at version thresholds, so all possible streams take linear-many views.
    fn views(&self) -> impl Iterator<Item = Vec<Token>> + '_ {
        [false, true].into_iter().flat_map(move |maria| {
            self.versions.iter().map(move |&version| {
                self.parts
                    .iter()
                    .filter(|(_, guard)| {
                        guard.is_none_or(|(only_maria, minimum)| {
                            (!only_maria || maria)
                                && minimum <= version
                                && !(maria && !only_maria && (50700..=99999).contains(&minimum))
                        })
                    })
                    .map(|(t, _)| t.clone())
                    .collect()
            })
        })
    }
}

fn hint(whole: &str, offset: usize) -> Result<(), String> {
    let toks = fragment(&whole[3..whole.len() - 2], offset + 3)?;
    for group in toks.windows(4) {
        let [set, open, name, equals] = group else { continue };
        let var = match &name.tok {
            Tok::Word(w) | Tok::Quoted(w) => w,
            Tok::Num(_) | Tok::Str(_) | Tok::Hint(_) | Tok::Exec(_) | Tok::Sym(_) => continue,
        };
        if matches!(&set.tok, Tok::Word(w) if w.eq_ignore_ascii_case("SET_VAR"))
            && open.tok == Tok::Sym("(")
            && equals.tok == Tok::Sym("=")
            && ["sql_mode", "sql_select_limit", "transaction_read_only", "tx_read_only", "max_execution_time"]
                .iter()
                .any(|w| var.eq_ignore_ascii_case(w))
        {
            return Err(format!("SET_VAR({var}) changes the read session's boundary"));
        }
    }
    Ok(())
}

// A hint's MAX_EXECUTION_TIME replaces the session's: 0 lifts it, and a longer one outlasts the call.
fn hint_time(whole: &str, offset: usize, timeout_ms: u64) -> Result<(), String> {
    let toks = fragment(&whole[3..whole.len() - 2], offset + 3)?;
    for (n, t) in toks.iter().enumerate() {
        if !matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("MAX_EXECUTION_TIME")) {
            continue;
        }
        let ms = match toks.get(n + 1..n + 4).map(|g| (&g[0].tok, &g[1].tok, &g[2].tok)) {
            Some((Tok::Sym("("), Tok::Num(ms), Tok::Sym(")"))) => ms.parse::<u64>().ok(),
            _ => None,
        };
        if !ms.is_some_and(|ms| (1..=timeout_ms).contains(&ms)) {
            return Err(format!(
                "MAX_EXECUTION_TIME would outlast the call: the server ends a read at its timeout, {timeout_ms} ms, and a hint may only lower it"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refuses(queries: &[&str]) {
        for q in queries {
            assert!(read(q).is_err(), "read: {q}");
        }
    }

    #[test]
    fn a_plain_explain_plans_a_locking_read_only() {
        for q in [
            "EXPLAIN SELECT a FROM t FOR UPDATE",
            "EXPLAIN SELECT a FROM t FOR SHARE",
            "EXPLAIN SELECT a FROM t LOCK IN SHARE MODE",
            "EXPLAIN ANALYZE SELECT a FROM t",
        ] {
            assert!(read(q).is_ok(), "refused: {q}");
        }
        refuses(&["SELECT a FROM t FOR UPDATE", "SELECT a FROM t FOR SHARE", "SELECT a FROM t LOCK IN SHARE MODE"]);
    }

    #[test]
    fn writes_stay_refused_plain_or_planned() {
        refuses(&[
            "INSERT INTO t (a) VALUES (1)",
            "REPLACE INTO t (a) VALUES (1)",
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "EXPLAIN INSERT INTO t (a) VALUES (1)",
            "EXPLAIN REPLACE INTO t (a) VALUES (1)",
            "EXPLAIN UPDATE t SET a = 1",
            "EXPLAIN DELETE FROM t",
            "EXPLAIN FORMAT=JSON DELETE FROM t",
            "/*!80018 EXPLAIN */ DELETE FROM t",
            "EXPLAIN SELECT a FROM t INTO OUTFILE '/x'",
        ]);
    }

    #[test]
    fn explain_analyze_of_locks_and_writes_stays_refused() {
        refuses(&[
            "EXPLAIN ANALYZE DELETE FROM t",
            "EXPLAIN ANALYZE UPDATE t SET a = 1",
            "EXPLAIN ANALYZE SELECT a FROM t FOR UPDATE",
            "EXPLAIN ANALYZE SELECT a FROM t LOCK IN SHARE MODE",
            "EXPLAIN /*!80018 ANALYZE */ SELECT a FROM t FOR UPDATE",
            "EXPLAIN /*M!100000 ANALYZE */ SELECT a FROM t FOR SHARE",
            "/*!80018 EXPLAIN */ SELECT a FROM t FOR UPDATE",
            "SELECT 1 /*!50000 FOR UPDATE */",
        ]);
    }
}
