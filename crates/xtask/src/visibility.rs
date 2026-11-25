//! Which doc comments sit on an item marked `pub`: the declaration's own visibility, never reach.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Attribute, Item, Visibility};

/// The lines of the doc comments a public declaration owns; `module_public` is the visibility of
/// the file's own module, which owns its `//!` lines.
pub fn public_doc_lines(file: &syn::File, module_public: bool) -> HashSet<usize> {
    let mut owners = Owners::default();
    if module_public {
        owners.take(&file.attrs);
    }
    owners.visit_file(file);
    owners.lines
}

/// Each module file's declared visibility, by the `mod` declaration that names it in the
/// parsed files; a file nothing declares is public only as a library's root.
pub fn module_files(files: &[(PathBuf, syn::File)]) -> HashMap<PathBuf, bool> {
    let mut out = HashMap::new();
    for (path, file) in files {
        let here = path.parent().unwrap_or(Path::new(""));
        declared(&file.items, &child_dirs(path), here, &mut out);
    }
    for (path, _) in files {
        out.entry(path.clone()).or_insert_with(|| path.file_name().is_some_and(|n| n == "lib.rs"));
    }
    out
}

fn is_pub(vis: &Visibility) -> bool {
    !matches!(vis, Visibility::Inherited)
}

// A crate root and `mod.rs` hold their children beside them, so a root file is told by its name or folder.
fn child_dirs(path: &Path) -> Vec<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let folder = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let root = matches!(stem, "lib" | "main" | "mod" | "build") || matches!(folder, "tests" | "bin" | "examples");
    vec![if root { parent.to_path_buf() } else { parent.join(stem) }]
}

// `here` is where a `#[path]` is read from: the file's folder, or an inline module's own folder.
fn declared(items: &[Item], dirs: &[PathBuf], here: &Path, out: &mut HashMap<PathBuf, bool>) {
    for item in items {
        let Item::Mod(m) = item else { continue };
        let name = m.ident.to_string();
        if let Some((_, inner)) = &m.content {
            let nested: Vec<PathBuf> = dirs.iter().map(|d| d.join(&name)).collect();
            let inner_here = nested.first().cloned().unwrap_or_default();
            declared(inner, &nested, &inner_here, out);
            continue;
        }
        if let Some(file) = path_attr(&m.attrs) {
            out.insert(normal(&here.join(file)), is_pub(&m.vis));
            continue;
        }
        for dir in dirs {
            out.insert(dir.join(format!("{name}.rs")), is_pub(&m.vis));
            out.insert(dir.join(&name).join("mod.rs"), is_pub(&m.vis));
        }
    }
}

fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().filter(|a| a.path().is_ident("path")).find_map(|a| match &a.meta.require_name_value().ok()?.value {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
        _ => None,
    })
}

// The walked files' paths hold no `..`, so a `#[path]` that climbs is folded to match them.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

// A doc comment's `[doc = …]` group, inner (`#![…]`) or outer, carries the comment's own span.
fn token_docs(tokens: proc_macro2::TokenStream, out: &mut HashSet<usize>) {
    use proc_macro2::{Delimiter, TokenTree};
    for tree in tokens {
        if let TokenTree::Group(g) = tree {
            let doc = g.delimiter() == Delimiter::Bracket
                && matches!(g.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "doc");
            if doc {
                out.insert(g.span().start().line);
            }
            token_docs(g.stream(), out);
        }
    }
}

#[derive(Default)]
struct Owners {
    lines: HashSet<usize>,
}

impl Owners {
    fn take(&mut self, attrs: &[Attribute]) {
        let docs = attrs.iter().filter(|a| a.path().is_ident("doc"));
        self.lines.extend(docs.map(|a| a.pound_token.spans[0].start().line));
    }

    fn take_if(&mut self, vis: &Visibility, attrs: &[Attribute]) {
        if is_pub(vis) {
            self.take(attrs);
        }
    }

    // A variant's fields have no visibility of their own: they take their enum's.
    fn take_fields<'a>(&mut self, fields: impl IntoIterator<Item = &'a syn::Field>, inherited: bool) {
        for f in fields {
            if inherited || is_pub(&f.vis) {
                self.take(&f.attrs);
            }
        }
    }

    fn take_enum(&mut self, e: &syn::ItemEnum) {
        for v in &e.variants {
            self.take(&v.attrs);
            self.take_fields(&v.fields, true);
        }
    }

    fn take_trait(&mut self, t: &syn::ItemTrait) {
        for item in &t.items {
            self.take(match item {
                syn::TraitItem::Fn(f) => &f.attrs,
                syn::TraitItem::Const(c) => &c.attrs,
                syn::TraitItem::Type(t) => &t.attrs,
                _ => &[],
            });
        }
    }

    fn take_impl(&mut self, i: &syn::ItemImpl) {
        for item in &i.items {
            match item {
                syn::ImplItem::Fn(f) => self.take_if(&f.vis, &f.attrs),
                syn::ImplItem::Const(c) => self.take_if(&c.vis, &c.attrs),
                syn::ImplItem::Type(t) => self.take_if(&t.vis, &t.attrs),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Owners {
    fn visit_item(&mut self, item: &'ast Item) {
        match item {
            Item::Const(i) => self.take_if(&i.vis, &i.attrs),
            Item::ExternCrate(i) => self.take_if(&i.vis, &i.attrs),
            Item::Fn(i) => self.take_if(&i.vis, &i.attrs),
            Item::Mod(i) => self.take_if(&i.vis, &i.attrs),
            Item::Static(i) => self.take_if(&i.vis, &i.attrs),
            Item::TraitAlias(i) => self.take_if(&i.vis, &i.attrs),
            Item::Type(i) => self.take_if(&i.vis, &i.attrs),
            Item::Use(i) => self.take_if(&i.vis, &i.attrs),
            Item::Struct(i) => {
                self.take_if(&i.vis, &i.attrs);
                self.take_fields(&i.fields, false);
            }
            Item::Union(i) => {
                self.take_if(&i.vis, &i.attrs);
                self.take_fields(&i.fields.named, false);
            }
            Item::Enum(i) if is_pub(&i.vis) => {
                self.take(&i.attrs);
                self.take_enum(i);
            }
            Item::Trait(i) if is_pub(&i.vis) => {
                self.take(&i.attrs);
                self.take_trait(i);
            }
            Item::Impl(i) if i.trait_.is_none() => self.take_impl(i),
            // A macro has no visibility: `#[macro_export]` is what makes its own docs public.
            Item::Macro(i) if i.attrs.iter().any(|a| a.path().is_ident("macro_export")) => self.take(&i.attrs),
            _ => {}
        }
        visit::visit_item(self, item);
    }

    // A macro's body is tokens, not items: what it expands to is unknown, so its docs pass (fac-195).
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        token_docs(m.tokens.clone(), &mut self.lines);
        visit::visit_macro(self, m);
    }
}
