use std::collections::HashSet;

use super::TscError;
use super::classes::{StringPolicy, check_segment};
use super::scan::{
    is_class_attr, is_expr_punct, next_is_condition_op, scan_block_comment, scan_quoted, scan_regex, scan_tag_head,
    skip_escape,
};

const EXPR_KEYWORDS: [&str; 11] =
    ["return", "case", "throw", "else", "typeof", "void", "delete", "in", "of", "yield", "await"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Template,
    JsxTag,
    JsxText,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Brace {
    Plain,
    Template,
    JsxAttr { class: bool },
    JsxChild,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    ExprStart,
    Arrow,
    Other,
}

struct Deferred<'a> {
    line: u64,
    text: &'a str,
    binding: Option<String>,
}

pub(super) fn scan_ts(rel: &str, text: &str) -> Vec<TscError> {
    TsLexer::new(rel, text).run()
}

struct TsLexer<'a> {
    rel: &'a str,
    text: &'a str,
    lines: Vec<&'a str>,
    jsx: bool,
    i: usize,
    line: u64,
    mode: Mode,
    prev: Prev,
    jsx_depth: usize,
    attr_is_class: bool,
    expect_decl: bool,
    decls: Vec<(String, usize)>,
    scopes: Vec<(Brace, usize)>,
    class_refs: HashSet<String>,
    non_class_refs: HashSet<String>,
    var_deps: Vec<(String, String)>,
    deferred: Vec<Deferred<'a>>,
    out: Vec<TscError>,
}

impl<'a> TsLexer<'a> {
    fn new(rel: &'a str, text: &'a str) -> Self {
        Self {
            rel,
            text,
            lines: text.lines().collect(),
            jsx: rel.ends_with(".tsx") || rel.ends_with(".jsx"),
            i: 0,
            line: 1,
            mode: Mode::Code,
            prev: Prev::ExprStart,
            jsx_depth: 0,
            attr_is_class: false,
            expect_decl: false,
            decls: Vec::new(),
            scopes: Vec::new(),
            class_refs: HashSet::new(),
            non_class_refs: HashSet::new(),
            var_deps: Vec::new(),
            deferred: Vec::new(),
            out: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<TscError> {
        let len = self.text.len();
        while self.i < len {
            match self.mode {
                Mode::Code => self.step_code(),
                Mode::Template => self.step_template(),
                Mode::JsxTag => self.step_jsx_tag(),
                Mode::JsxText => self.step_jsx_text(),
            }
        }
        self.resolve_deferred();
        self.out.sort_by_key(|e| e.line);
        self.out
    }

    fn resolve_deferred(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            for (lhs, rhs) in &self.var_deps {
                if self.class_refs.contains(lhs) && self.class_refs.insert(rhs.clone()) {
                    changed = true;
                }
            }
        }
        for d in &self.deferred {
            let policy = match d.binding.as_deref() {
                Some(name) if self.class_refs.contains(name) => StringPolicy::AlwaysClass,
                Some(name) if self.non_class_refs.contains(name) => StringPolicy::Skip,
                _ => StringPolicy::Heuristic,
            };
            check_segment(self.rel, &self.lines, d.line, d.text, policy, &mut self.out);
        }
    }

    fn string_policy(&self) -> StringPolicy {
        for (brace, _) in self.scopes.iter().rev() {
            match brace {
                Brace::Template => {}
                Brace::JsxAttr { class: true } => return StringPolicy::AlwaysClass,
                Brace::JsxAttr { class: false } | Brace::JsxChild => return StringPolicy::Skip,
                Brace::Plain => return StringPolicy::Heuristic,
            }
        }
        StringPolicy::Heuristic
    }

    fn push_brace(&mut self, brace: Brace) {
        self.scopes.push((brace, self.jsx_depth));
        self.jsx_depth = 0;
        self.mode = Mode::Code;
        self.prev = Prev::ExprStart;
        self.expect_decl = false;
    }

    fn pop_brace(&mut self) {
        let Some((brace, depth)) = self.scopes.pop() else { return };
        self.decls.retain(|(_, d)| *d <= self.scopes.len());
        self.jsx_depth = depth;
        self.mode = match brace {
            Brace::Plain => Mode::Code,
            Brace::Template => Mode::Template,
            Brace::JsxAttr { .. } => Mode::JsxTag,
            Brace::JsxChild => Mode::JsxText,
        };
        self.prev = Prev::Other;
        self.expect_decl = false;
    }

    fn skip_comment(&mut self) -> bool {
        let bytes = self.text.as_bytes();
        if bytes.get(self.i..self.i + 2) == Some(b"/*") {
            let (next, nl) = scan_block_comment(bytes, self.i);
            self.i = next;
            self.line += nl;
            return true;
        }
        if bytes.get(self.i..self.i + 2) == Some(b"//") {
            while self.i < bytes.len() && bytes[self.i] != b'\n' {
                self.i += 1;
            }
            return true;
        }
        false
    }

    fn handle_ident(&mut self, ident: &str) {
        if matches!(ident, "const" | "let" | "var" | "function") {
            self.clear_scope_decl();
            self.expect_decl = true;
            self.prev = Prev::Other;
            return;
        }
        if matches!(ident, "export" | "return") {
            self.clear_scope_decl();
        }
        if self.expect_decl {
            self.expect_decl = false;
            self.decls.push((ident.to_owned(), self.scopes.len()));
            self.prev = Prev::Other;
            return;
        }
        match self.string_policy() {
            StringPolicy::AlwaysClass if !next_is_condition_op(self.text.as_bytes(), self.i) => {
                self.class_refs.insert(ident.to_owned());
            }
            StringPolicy::Skip => {
                self.non_class_refs.insert(ident.to_owned());
            }
            StringPolicy::AlwaysClass | StringPolicy::Heuristic => {
                if let Some((lhs, _)) = self.decls.last()
                    && lhs != ident
                {
                    self.var_deps.push((lhs.clone(), ident.to_owned()));
                }
            }
        }
        self.prev = if EXPR_KEYWORDS.contains(&ident) { Prev::ExprStart } else { Prev::Other };
    }

    fn clear_scope_decl(&mut self) {
        let cur = self.scopes.len();
        while self.decls.last().is_some_and(|(_, d)| *d == cur) {
            self.decls.pop();
        }
    }

    fn step_code(&mut self) {
        if self.skip_comment() {
            return;
        }
        let bytes = self.text.as_bytes();
        match bytes[self.i] {
            b'\n' => {
                self.line += 1;
                self.i += 1;
            }
            b if b.is_ascii_whitespace() => self.i += 1,
            b'=' if bytes.get(self.i + 1) == Some(&b'>') => {
                self.i += 2;
                self.prev = Prev::Arrow;
                self.expect_decl = false;
            }
            b'/' if self.prev == Prev::ExprStart => {
                self.i = scan_regex(bytes, self.i).map_or(self.i + 1, |end| end + 1);
                self.prev = Prev::Other;
            }
            b'<' if self.jsx && matches!(self.prev, Prev::ExprStart | Prev::Arrow) => self.try_open_jsx(),
            b'"' => self.take_quoted(b'"', false, self.string_policy()),
            b'\'' if self.i == 0 || !bytes[self.i - 1].is_ascii_alphanumeric() => {
                self.take_quoted(b'\'', false, self.string_policy());
            }
            b'`' => {
                self.mode = Mode::Template;
                self.i += 1;
            }
            b'{' => {
                self.push_brace(Brace::Plain);
                self.i += 1;
            }
            b'}' => {
                self.pop_brace();
                self.i += 1;
            }
            b';' => {
                self.clear_scope_decl();
                self.expect_decl = false;
                self.prev = Prev::ExprStart;
                self.i += 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                let start = self.i;
                while self.i < bytes.len()
                    && (bytes[self.i].is_ascii_alphanumeric() || matches!(bytes[self.i], b'_' | b'$'))
                {
                    self.i += 1;
                }
                self.handle_ident(&self.text[start..self.i]);
            }
            b => {
                self.expect_decl = false;
                self.prev = if is_expr_punct(b) { Prev::ExprStart } else { Prev::Other };
                self.i += 1;
            }
        }
    }

    fn try_open_jsx(&mut self) {
        let bytes = self.text.as_bytes();
        if bytes.get(self.i + 1) == Some(&b'>') {
            self.i += 2;
            self.jsx_depth += 1;
            self.mode = Mode::JsxText;
        } else if let Some(after) = scan_tag_head(bytes, self.i) {
            self.i = after;
            self.attr_is_class = false;
            self.mode = Mode::JsxTag;
        } else {
            self.prev = Prev::Other;
            self.i += 1;
        }
    }

    fn step_jsx_tag(&mut self) {
        if self.skip_comment() {
            return;
        }
        let bytes = self.text.as_bytes();
        match bytes[self.i] {
            b'\n' => {
                self.line += 1;
                self.i += 1;
            }
            b'/' if bytes.get(self.i + 1) == Some(&b'>') => {
                self.i += 2;
                self.attr_is_class = false;
                self.mode = if self.jsx_depth > 0 { Mode::JsxText } else { Mode::Code };
                self.prev = Prev::Other;
            }
            b'>' => {
                self.i += 1;
                self.attr_is_class = false;
                self.jsx_depth += 1;
                self.mode = Mode::JsxText;
            }
            q @ (b'"' | b'\'') => {
                let policy = if self.attr_is_class { StringPolicy::AlwaysClass } else { StringPolicy::Skip };
                self.attr_is_class = false;
                self.take_quoted(q, true, policy);
            }
            b'{' => {
                let class = self.attr_is_class;
                self.attr_is_class = false;
                self.push_brace(Brace::JsxAttr { class });
                self.i += 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = self.i;
                while self.i < bytes.len()
                    && (bytes[self.i].is_ascii_alphanumeric() || matches!(bytes[self.i], b'_' | b'-' | b':'))
                {
                    self.i += 1;
                }
                self.attr_is_class = is_class_attr(&self.text[start..self.i]);
            }
            _ => self.i += 1,
        }
    }

    fn step_jsx_text(&mut self) {
        let bytes = self.text.as_bytes();
        match bytes[self.i] {
            b'\n' => {
                self.line += 1;
                self.i += 1;
            }
            b'{' => {
                self.push_brace(Brace::JsxChild);
                self.i += 1;
            }
            b'<' if bytes.get(self.i + 1) == Some(&b'/') => self.close_jsx_tag(),
            b'<' => self.try_open_jsx(),
            _ => self.i += 1,
        }
    }

    fn close_jsx_tag(&mut self) {
        let bytes = self.text.as_bytes();
        self.i += 2;
        while self.i < bytes.len() && bytes[self.i] != b'>' {
            if bytes[self.i] == b'\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        if self.i < bytes.len() {
            self.i += 1;
        }
        self.jsx_depth = self.jsx_depth.saturating_sub(1);
        if self.jsx_depth == 0 {
            self.mode = Mode::Code;
            self.prev = Prev::Other;
        }
    }

    fn record_segment(&mut self, start_line: u64, slice: &'a str, policy: StringPolicy) {
        if policy == StringPolicy::Heuristic {
            let binding = self.decls.last().map(|(name, _)| name.clone());
            self.deferred.push(Deferred { line: start_line, text: slice, binding });
        } else {
            check_segment(self.rel, &self.lines, start_line, slice, policy, &mut self.out);
        }
    }

    fn take_quoted(&mut self, quote: u8, multiline: bool, policy: StringPolicy) {
        if let Some((end, nl)) = scan_quoted(self.text.as_bytes(), self.i, quote, multiline) {
            self.record_segment(self.line, &self.text[self.i + 1..end], policy);
            self.line += nl;
            self.i = end + 1;
            self.prev = Prev::Other;
        } else {
            self.i += 1;
        }
    }

    fn step_template(&mut self) {
        let bytes = self.text.as_bytes();
        let start = self.i;
        let start_line = self.line;
        let policy = self.string_policy();
        while self.i < bytes.len() {
            match bytes[self.i] {
                b'\\' => {
                    if bytes.get(self.i + 1) == Some(&b'\n') {
                        self.line += 1;
                    }
                    self.i += skip_escape(bytes, self.i);
                }
                b'`' => {
                    self.record_segment(start_line, &self.text[start..self.i], policy);
                    self.mode = Mode::Code;
                    self.prev = Prev::Other;
                    self.i += 1;
                    return;
                }
                b'$' if bytes.get(self.i + 1) == Some(&b'{') => {
                    self.record_segment(start_line, &self.text[start..self.i], policy);
                    self.push_brace(Brace::Template);
                    self.i += 2;
                    return;
                }
                b'\n' => {
                    self.line += 1;
                    self.i += 1;
                }
                _ => self.i += 1,
            }
        }
        self.record_segment(start_line, &self.text[start..self.i], policy);
    }
}
