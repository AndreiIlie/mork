// MySQL tokens with backslash escapes, matching the door connection's lexical mode (fac-124).
// Hints and executable comments stay visible to the read gate.

use std::ops::Range;

// The longest query read.
const MAX_QUERY: usize = 16 * 1024;

/// One token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// A bare word: a keyword or a name, as written; an integer running into letters is one.
    Word(String),
    /// A name in backticks.
    Quoted(String),
    /// A number, as written: digits, a fraction, an exponent.
    Num(String),
    /// A string's value.
    Str(String),
    /// An optimizer hint `/*+ … */`, kept whole: the server reads it, never as a statement.
    Hint(String),
    /// Version-gated SQL, retained for the read gate to inspect.
    Exec(String),
    /// An operator or a punctuation mark.
    Sym(&'static str),
}

/// A token and where it stands in the query; `spaced` when whitespace or a comment precedes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tok: Tok,
    pub at: Range<usize>,
    pub spaced: bool,
}

const SYMS: [&str; 33] = [
    "<=>", "->>", "<<", ">>", "<=", ">=", "<>", "!=", ":=", "->", "&&", "||", "<", ">", "=", "(", ")", ",", ".", "*",
    "+", "-", "/", "%", ";", "&", "|", "^", "~", "!", ":", "{", "}",
];

/// The query's tokens, comments dropped and a trailing `;` with them; or why it can't be read.
pub(super) fn tokens(q: &str) -> Result<Vec<Token>, String> {
    if q.len() > MAX_QUERY {
        return Err(format!("the query is over {MAX_QUERY} bytes"));
    }
    let mut out = fragment(q, 0)?;
    if out.last().is_some_and(|t| t.tok == Tok::Sym(";")) {
        out.pop();
    }
    if out.iter().any(|t| t.tok == Tok::Sym(";")) {
        return Err("more than one statement: one read at a time".into());
    }
    Ok(out)
}

/// Reads a fragment starting after `offset` characters of the original query (fac-417).
pub(super) fn fragment(q: &str, offset: usize) -> Result<Vec<Token>, String> {
    walk(q, offset, false)
}

/// As `fragment`, for an executable comment's body: a line comment there must end inside it.
pub(super) fn executed(q: &str, offset: usize) -> Result<Vec<Token>, String> {
    walk(q, offset, true)
}

// A server running an executable comment reads a `#` or `-- ` comment in it on past its `*/`.
fn walk(q: &str, offset: usize, closed_lines: bool) -> Result<Vec<Token>, String> {
    let b = q.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let (mut i, mut spaced) = (0, false);
    while i < b.len() {
        if matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
            (i, spaced) = (i + 1, true);
            continue;
        }
        let located = |e| format!("{e} at character {}", offset + q[..i].chars().count() + 1);
        if let Some(end) = comment(q, i).map_err(located)? {
            if closed_lines && !q[i..].starts_with("/*") && !q[i..end].ends_with('\n') {
                return Err(located(
                    "a line comment in an executable comment isn't read: the server reads it past `*/`".into(),
                ));
            }
            (i, spaced) = (end, true);
            continue;
        }
        let (tok, next) = token(q, i).map_err(located)?;
        out.push(Token { tok, at: i..next, spaced });
        (i, spaced) = (next, false);
    }
    Ok(out)
}

// MySQL's three comments, `--` only before whitespace; `/*!`, `/*M!` and `/*+` are read by the
// server as SQL or hints, so none is dropped unseen.
fn comment(q: &str, i: usize) -> Result<Option<usize>, String> {
    let rest = &q[i..];
    let to_line = || rest.find('\n').map_or(q.len(), |n| i + n + 1);
    if rest.starts_with('#') {
        return Ok(Some(to_line()));
    }
    if rest.starts_with("--") && rest.as_bytes().get(2).is_none_or(|c| c.is_ascii_whitespace() || c.is_ascii_control())
    {
        return Ok(Some(to_line()));
    }
    let Some(body) = rest.strip_prefix("/*") else { return Ok(None) };
    // A hint and an executable comment are tokens the gate reads (`token`), not plain comments
    // dropped here: the server reads them, so neither may vanish unseen.
    if body.starts_with('+') || body.starts_with('!') || body.starts_with("M!") {
        return Ok(None);
    }
    let end = body.find("*/").ok_or("a comment isn't closed")?;
    Ok(Some(i + 2 + end + 2))
}

fn token(q: &str, i: usize) -> Result<(Tok, usize), String> {
    if q[i..].starts_with("/*+") {
        let end = q[i + 3..].find("*/").ok_or("an optimizer hint isn't closed")?;
        hint_body(&q[i + 3..i + 3 + end])?;
        let at = i + 3 + end + 2;
        return Ok((Tok::Hint(q[i..at].to_owned()), at));
    }
    if q[i..].starts_with("/*!") || q[i..].starts_with("/*M!") {
        let end = q[i + 2..].find("*/").ok_or("an executable comment isn't closed")?;
        // A server skipping a newer version's comment closes one nested `/*` first.
        if q[i + 2..i + 2 + end].contains("/*") {
            return Err("a comment inside an executable comment isn't read: the server may end it later".into());
        }
        let at = i + 2 + end + 2;
        return Ok((Tok::Exec(q[i..at].to_owned()), at));
    }
    let c = q[i..].chars().next().unwrap_or(' ');
    let next = q.as_bytes().get(i + 1).copied();
    match c {
        '\'' | '"' => string(q, i),
        '`' => quoted(q, i),
        '0'..='9' => Ok(number(q, i)),
        '.' if next.is_some_and(|n| n.is_ascii_digit()) => Ok(number(q, i)),
        '@' => Ok((Tok::Sym("@"), i + 1)),
        c if ident(c) => {
            let j = word_end(q, i);
            Ok((Tok::Word(q[i..j].to_owned()), j))
        }
        _ => sym(q, i),
    }
}

// MySQL's hint parser reads quotes, doubled but never escaped, and no comments: a `*/` inside one
// of its quotes doesn't end the hint, and the gate reads a hint's body only where both lexers agree.
fn hint_body(body: &str) -> Result<(), String> {
    if body.contains('\\') {
        return Err("a backslash in an optimizer hint isn't read: the server's hint parser reads no escapes".into());
    }
    let mut open = None;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match open {
            None if matches!(c, '\'' | '"' | '`') => open = Some(c),
            None if c == '#' || (c == '-' && chars.peek() == Some(&'-')) => {
                return Err("a comment in an optimizer hint isn't read: the server's hint parser reads none".into());
            }
            Some(o) if c == o => open = None,
            None | Some(_) => {}
        }
    }
    match open {
        Some(_) => Err("a quote in an optimizer hint isn't closed before its `*/`".into()),
        None => Ok(()),
    }
}

// MySQL's unquoted names: ASCII letters, digits, `_`, `$`, and any character beyond ASCII.
fn ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || !c.is_ascii()
}

fn word_end(q: &str, i: usize) -> usize {
    q[i..].char_indices().find(|(_, c)| !ident(*c)).map_or(q.len(), |(n, _)| i + n)
}

fn sym(q: &str, i: usize) -> Result<(Tok, usize), String> {
    let rest = &q[i..];
    if let Some(s) = SYMS.iter().find(|s| rest.starts_with(**s)) {
        return Ok((Tok::Sym(s), i + s.len()));
    }
    let ch = rest.chars().next().unwrap_or(' ');
    if ch == '\\' {
        return Err("a backslash isn't read: the mysql client would read it as its own command".into());
    }
    Err(format!("`{}` isn't read in a query", ch.escape_debug()))
}

// As MySQL reads it: an integer running into a name's characters (`1abc`, `0x1f`) is a name to the
// gate, while a fraction or an exponent ends the number before a keyword (`1.5FOR`, `1e0INTO`).
fn number(q: &str, i: usize) -> (Tok, usize) {
    let b = q.as_bytes();
    let digits = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let mut j = digits(i);
    let integer = j;
    if b.get(j) == Some(&b'.') {
        j = digits(j + 1);
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let k = if matches!(b.get(j + 1), Some(b'+' | b'-')) { j + 2 } else { j + 1 };
        if b.get(k).is_some_and(u8::is_ascii_digit) {
            j = digits(k);
        }
    }
    if j == integer && q[j..].chars().next().is_some_and(ident) {
        let end = word_end(q, j);
        return (Tok::Word(q[i..end].to_owned()), end);
    }
    (Tok::Num(q[i..j].to_owned()), j)
}

// The connection clears NO_BACKSLASH_ESCAPES and ANSI_QUOTES before the client reads this text.
fn string(q: &str, i: usize) -> Result<(Tok, usize), String> {
    let quote = char::from(q.as_bytes()[i]);
    let mut out = String::new();
    let mut chars = q[i + 1..].char_indices().peekable();
    while let Some((k, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, escaped) = chars.next().ok_or("a string isn't closed")?;
                if is_refused_control(escaped) {
                    return Err("a control character in a string isn't read".into());
                }
                out.push(escaped);
            }
            c if c == quote && chars.peek().map(|(_, n)| *n) == Some(quote) => {
                out.push(quote);
                chars.next();
            }
            c if c == quote => return Ok((Tok::Str(out), i + 1 + k + 1)),
            c if is_refused_control(c) => return Err("a control character in a string isn't read".into()),
            c => out.push(c),
        }
    }
    Err("a string isn't closed".into())
}

// A backslash is literal in a backtick name under every mode; a doubled backtick is the backtick.
fn quoted(q: &str, i: usize) -> Result<(Tok, usize), String> {
    let mut out = String::new();
    let mut chars = q[i + 1..].char_indices().peekable();
    while let Some((k, c)) = chars.next() {
        match c {
            '`' if chars.peek().map(|(_, n)| *n) == Some('`') => {
                out.push('`');
                chars.next();
            }
            '`' if out.is_empty() => return Err("a name in backticks is empty".into()),
            '`' => return Ok((Tok::Quoted(out), i + 1 + k + 1)),
            c if is_refused_control(c) => return Err("a control character in a name isn't read".into()),
            c => out.push(c),
        }
    }
    Err("a name in backticks isn't closed".into())
}

// A line break or a tab a literal may hold is read; any other control character (a NUL above all,
// which a C client reads as the string's end) is not.
fn is_refused_control(c: char) -> bool {
    c.is_control() && !matches!(c, '\t' | '\n' | '\r')
}

#[cfg(test)]
mod tests {
    // why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn toks(q: &str) -> Vec<Tok> {
        tokens(q).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn seeded_tokens_keep_their_utf8_spans() {
        let atoms = [
            ("SELECT", Tok::Word("SELECT".into())),
            ("'λ; it''s'", Tok::Str("λ; it's".into())),
            ("`a``b`", Tok::Quoted("a`b".into())),
            ("0xFA", Tok::Word("0xFA".into())),
            ("1.25e-3", Tok::Num("1.25e-3".into())),
            ("/*!80000 + 2 */", Tok::Exec("/*!80000 + 2 */".into())),
            (",", Tok::Sym(",")),
        ];
        for seed in 0..128 {
            eprintln!("lexer seed {seed}");
            let mut rng = fastrand::Rng::with_seed(seed);
            let mut query = String::new();
            let mut expected = Vec::new();
            for _ in 0..rng.usize(1..24) {
                query.push_str(rng.choice([" ", "\t", "/*雪*/", "-- comment\n"]).unwrap());
                let (raw, tok) = rng.choice(&atoms).unwrap();
                let start = query.len();
                query.push_str(raw);
                expected.push(Token { tok: tok.clone(), at: start..query.len(), spaced: true });
            }
            query.push(';');
            assert_eq!(tokens(&query).unwrap(), expected, "seed {seed}");

            let boundaries: Vec<_> = query.char_indices().map(|(at, _)| at).collect();
            let start = boundaries[rng.usize(..boundaries.len())];
            query.truncate(start);
            for _ in 0..rng.usize(1..20) {
                query.push(rng.choice(['\'', '"', '\\', '\0', ';', '/', '*', '雪']).unwrap());
            }
            if let Ok(parsed) = tokens(&query) {
                let mut end = 0;
                for t in parsed {
                    assert!(end <= t.at.start && !t.at.is_empty() && query.get(t.at.clone()).is_some(), "seed {seed}");
                    end = t.at.end;
                }
            }
        }
    }

    // fac-748 review r1 (finding 2): MySQL ends a fraction or an exponent at its last digit.
    #[test]
    fn fac748_a_fraction_or_an_exponent_ends_before_a_keyword() {
        let num = |n: &str| Tok::Num(n.into());
        let word = |w: &str| Tok::Word(w.into());
        assert_eq!(
            toks("SELECT 1.0INTO, .5FOR, 1e+0LOCK, 1.e5x")[1..],
            [num("1.0"), word("INTO"), Tok::Sym(","), num(".5"), word("FOR"), Tok::Sym(",")]
                .into_iter()
                .chain([num("1e+0"), word("LOCK"), Tok::Sym(","), num("1.e5"), word("x")])
                .collect::<Vec<_>>()
        );
    }

    // Unclosed literals or a NUL could hide a statement boundary from the client or the gate.
    #[test]
    fn fac124_no_word_hides_from_the_gate() {
        for q in [
            "SELECT /*+ BKA(t) ", // an unclosed hint
            "SELECT /*! 1 ",      // an unclosed executable comment
            "SELECT 1 /* open",
            "SELECT 1 \\! ls", // a bare backslash: a client command
            "SELECT 'a\0b'",   // a NUL: a C client's string end
            "SELECT 1; DROP TABLE t",
            "SELECT 1;;",
        ] {
            assert!(tokens(q).is_err(), "read: {q}");
        }
        assert_eq!(toks("SELECT 1 -- INTO\n, 2 # INTO\n"), toks("SELECT 1, 2"));
        assert_eq!(
            toks("SELECT /*+ MAX_EXECUTION_TIME(1000) */ 1")[1],
            Tok::Hint("/*+ MAX_EXECUTION_TIME(1000) */".into()),
            "an optimizer hint is one token, kept whole"
        );
        assert_eq!(
            toks("SELECT /*!40101 INTO OUTFILE '/x' */ 1")[1],
            Tok::Exec("/*!40101 INTO OUTFILE '/x' */".into()),
            "an executable comment is one token, its body left for the gate to read"
        );
        assert_eq!(toks("SELECT 'C:\\path', 'a\nb\tc'")[1], Tok::Str("C:path".into()));
        assert_eq!(toks("SELECT `a\\b`.`c`")[1], Tok::Quoted("a\\b".into()));
        assert_eq!(
            toks("SELECT 1 --1"),
            [Tok::Word("SELECT".into()), Tok::Num("1".into()), Tok::Sym("-"), Tok::Sym("-"), Tok::Num("1".into())]
        );
        assert_eq!(
            toks("SELECT 1INTO, 0x1f, 1e5")[1..],
            [Tok::Word("1INTO".into()), Tok::Sym(","), Tok::Word("0x1f".into()), Tok::Sym(","), Tok::Num("1e5".into()),]
        );
        assert_eq!(
            toks("SELECT `a``b`, 'it''s'")[1..],
            [Tok::Quoted("a`b".into()), Tok::Sym(","), Tok::Str("it's".into())]
        );
    }
}
