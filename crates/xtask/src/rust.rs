//! Rust sources read as syntax (`syn`) with `use` aliases resolved; `#[cfg(test)]` modules are left out.

use std::collections::HashMap;

use syn::visit::{self, Visit};

/// One thing a file names or does, at a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// A path, resolved through the file's `use` aliases: `std::time::SystemTime::now`.
    Path(String),
    /// A method call by name: `elapsed`, `spawn`, `exec`.
    Method(String),
    Unsafe,
    Async,
    /// The tokens of a macro's body, which `syn` doesn't parse: read as text.
    Tokens(String),
}

/// What a file holds: each `Seen`, with its line.
pub fn scan(text: &str) -> Result<Vec<(usize, Seen)>, String> {
    let file = syn::parse_file(text).map_err(|e| e.to_string())?;
    let mut v = Scanner { scopes: vec![aliases_of(file.items.iter())], seen: Vec::new() };
    v.visit_file(&file);
    Ok(v.seen)
}

// M1.32: a block's or an inline module's aliases too.
pub(crate) fn aliases_of<'a>(items: impl Iterator<Item = &'a syn::Item>) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for item in items {
        if let syn::Item::Use(u) = item
            && !is_test(&u.attrs)
        {
            collect_aliases(&u.tree, String::new(), &mut aliases);
        }
    }
    aliases
}

pub fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && a.meta.require_list().is_ok_and(|l| l.tokens.to_string() == "test"))
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_owned() } else { format!("{prefix}::{name}") }
}

fn collect_aliases(tree: &syn::UseTree, prefix: String, out: &mut HashMap<String, String>) {
    match tree {
        syn::UseTree::Path(p) => collect_aliases(&p.tree, join(&prefix, &p.ident.to_string()), out),
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            let full = if name == "self" { prefix.clone() } else { join(&prefix, &name) };
            let short = if name == "self" { prefix.rsplit("::").next().unwrap_or("").to_owned() } else { name };
            out.insert(short, full);
        }
        syn::UseTree::Rename(r) => {
            let name = r.ident.to_string();
            let full = if name == "self" { prefix.clone() } else { join(&prefix, &name) };
            out.insert(r.rename.to_string(), full);
        }
        syn::UseTree::Glob(_) => {}
        syn::UseTree::Group(g) => {
            for t in &g.items {
                collect_aliases(t, prefix.clone(), out);
            }
        }
    }
}

fn use_paths(tree: &syn::UseTree, prefix: String, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(p) => use_paths(&p.tree, join(&prefix, &p.ident.to_string()), out),
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            out.push(if name == "self" { prefix } else { join(&prefix, &name) });
        }
        syn::UseTree::Rename(r) => {
            let name = r.ident.to_string();
            out.push(if name == "self" { prefix } else { join(&prefix, &name) });
        }
        syn::UseTree::Glob(_) => out.push(join(&prefix, "*")),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                use_paths(t, prefix.clone(), out);
            }
        }
    }
}

struct Scanner {
    // Outermost first: the file's, then each block or module entered.
    scopes: Vec<HashMap<String, String>>,
    seen: Vec<(usize, Seen)>,
}

fn line_of(span: proc_macro2::Span) -> usize {
    span.start().line
}

impl Scanner {
    fn alias(&self, name: &str) -> Option<String> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn scoped(&mut self, aliases: HashMap<String, String>, f: impl FnOnce(&mut Self)) {
        self.scopes.push(aliases);
        f(self);
        self.scopes.pop();
    }

    // A macro body is only text, so each alias in scope is written out (`Clock::now` read as
    // `std::time::SystemTime::now`).
    fn expanded(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for scope in &self.scopes {
            for (short, full) in scope {
                out = replace_word(&out, &format!("{short}::"), &format!("{full}::"));
            }
        }
        out
    }

    fn resolve(&self, path: &syn::Path) -> String {
        let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let Some((first, rest)) = segs.split_first() else { return String::new() };
        let head = self.alias(first).unwrap_or_else(|| first.clone());
        let mut all = vec![head];
        all.extend(rest.iter().cloned());
        all.join("::")
    }
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !is_test(&m.attrs) {
            let aliases = aliases_of(m.content.iter().flat_map(|(_, items)| items));
            self.scoped(aliases, |v| visit::visit_item_mod(v, m));
        }
    }
    fn visit_block(&mut self, b: &'ast syn::Block) {
        let items = b.stmts.iter().filter_map(|s| if let syn::Stmt::Item(i) = s { Some(i) } else { None });
        let aliases = aliases_of(items);
        self.scoped(aliases, |v| visit::visit_block(v, b));
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        if is_test(&f.attrs) {
            return;
        }
        if f.sig.unsafety.is_some() {
            self.seen.push((line_of(f.sig.fn_token.span), Seen::Unsafe));
        }
        if f.sig.asyncness.is_some() {
            self.seen.push((line_of(f.sig.fn_token.span), Seen::Async));
        }
        visit::visit_item_fn(self, f);
    }
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        if is_test(&u.attrs) {
            return;
        }
        let mut paths = Vec::new();
        use_paths(&u.tree, String::new(), &mut paths);
        let line = line_of(u.use_token.span);
        self.seen.extend(paths.into_iter().map(|p| (line, Seen::Path(p))));
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if let Some(u) = i.unsafety {
            self.seen.push((line_of(u.span), Seen::Unsafe));
        }
        visit::visit_item_impl(self, i);
    }
    fn visit_expr_unsafe(&mut self, e: &'ast syn::ExprUnsafe) {
        self.seen.push((line_of(e.unsafe_token.span), Seen::Unsafe));
        visit::visit_expr_unsafe(self, e);
    }
    fn visit_expr_async(&mut self, e: &'ast syn::ExprAsync) {
        self.seen.push((line_of(e.async_token.span), Seen::Async));
        visit::visit_expr_async(self, e);
    }
    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.seen.push((line_of(m.method.span()), Seen::Method(m.method.to_string())));
        visit::visit_expr_method_call(self, m);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let text = self.expanded(&m.tokens.to_string().replace(' ', ""));
        self.seen.push((
            line_of(m.path.segments.first().map_or_else(proc_macro2::Span::call_site, |s| s.ident.span())),
            Seen::Tokens(text),
        ));
        // A body of expressions (`vec![…]`, `format!(…)`) is read as syntax too, its paths
        // resolved through the aliases in scope.
        let exprs = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(list) = m.parse_body_with(exprs) {
            for e in &list {
                self.visit_expr(e);
            }
        }
        visit::visit_macro(self, m);
    }
    fn visit_path(&mut self, p: &'ast syn::Path) {
        if let Some(first) = p.segments.first() {
            self.seen.push((line_of(first.ident.span()), Seen::Path(self.resolve(p))));
        }
        visit::visit_path(self, p);
    }
}

// Only where `from` isn't preceded by a part of an identifier.
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(from) {
        let before = rest[..i].chars().next_back().or_else(|| out.chars().next_back());
        out.push_str(&rest[..i]);
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        rest = &rest[i + from.len()..];
    }
    out.push_str(rest);
    out
}
