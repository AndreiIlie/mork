// Only existing syntax needs justification. Whitespace, comments and line shifts have no tokens.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::ToTokens;
use syn::visit::{self, Visit};
use xtask::{rust, walk};

struct Item {
    syntax: String,
    line: usize,
}

#[derive(Default)]
struct Items {
    scope: Vec<String>,
    tests: bool,
    by_name: BTreeMap<String, Vec<Item>>,
}

impl Items {
    fn insert(&mut self, kind: &str, syntax: &impl ToTokens, line: usize) {
        let name = format!("{kind} in {}", self.scope.join("::"));
        self.by_name.entry(name).or_default().push(Item { syntax: syntax.to_token_stream().to_string(), line });
    }

    fn assertion(&mut self, name: &Ident, syntax: &impl ToTokens) {
        let ident = name.to_string();
        if matches!(ident.as_str(), "assert" | "debug_assert" | "debug_assert_eq" | "debug_assert_ne")
            || ident.starts_with("assert_")
        {
            self.insert(&format!("{name}!"), syntax, name.span().start().line);
        }
    }

    // syn leaves macro arguments opaque, even when they contain ordinary test code.
    fn nested_assertions(&mut self, stream: TokenStream) {
        let tokens: Vec<_> = stream.into_iter().collect();
        for (i, token) in tokens.iter().enumerate() {
            if let TokenTree::Group(group) = token {
                if let [TokenTree::Ident(name), TokenTree::Punct(bang)] = &tokens[i.saturating_sub(2)..i]
                    && bang.as_char() == '!'
                {
                    self.assertion(name, group);
                }
                self.nested_assertions(group.stream());
            }
        }
    }

    fn read(file: &str, source: &str) -> Result<Self, String> {
        let mut items = Self { tests: walk::is_test_file(Path::new(file)), ..Self::default() };
        let syntax = syn::parse_file(source).map_err(|e| format!("{file}: {e}"))?;
        items.tests |= rust::is_test(&syntax.attrs);
        items.visit_file(&syntax);
        Ok(items)
    }
}

impl<'ast> Visit<'ast> for Items {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let outer = self.tests;
        self.tests |= rust::is_test(&module.attrs);
        self.scope.push(module.ident.to_string());
        visit::visit_item_mod(self, module);
        self.scope.pop();
        self.tests = outer;
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let outer = self.tests;
        self.scope.push(function.sig.ident.to_string());
        let test = function.attrs.iter().any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"));
        if test {
            self.insert("test", function, function.sig.ident.span().start().line);
        }
        self.tests |= test || rust::is_test(&function.attrs);
        visit::visit_item_fn(self, function);
        self.tests = outer;
        self.scope.pop();
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.scope.push(item.self_ty.to_token_stream().to_string());
        visit::visit_item_impl(self, item);
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        self.scope.push(method.sig.ident.to_string());
        visit::visit_impl_item_fn(self, method);
        self.scope.pop();
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let Some(last) = mac.path.segments.last().filter(|_| self.tests) else { return };
        self.assertion(&last.ident, mac);
        self.nested_assertions(mac.tokens.clone());
    }
}

fn compare(old: &str, before: Items, file: &str, mut after: Items, flags: &mut Vec<String>) {
    for (name, items) in before.by_name {
        let mut remaining = after.by_name.remove(&name).unwrap_or_default();
        let changed = items
            .into_iter()
            .filter(|item| {
                if let Some(i) = remaining.iter().position(|new| new.syntax == item.syntax) {
                    remaining.remove(i);
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        let mut replacements = remaining.into_iter();
        for item in changed {
            let flag = match replacements.next() {
                Some(new) => format!("{file}:{}: modified {name}", new.line),
                None => format!("{old}:{} (base): deleted {name}", item.line),
            };
            flags.push(flag);
        }
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").current_dir(root).args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args[0], String::from_utf8_lossy(&out.stderr)));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

pub(super) fn against(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let raw = git(root, &["diff", "--raw", "--no-abbrev", "-z", "-M", "--diff-filter=DMRT", base, "--", "*.rs"])?;
    let mut fields = raw.split_terminator('\0');
    let mut flags = Vec::new();
    while let Some(header) = fields.next() {
        let words: Vec<_> = header.split_ascii_whitespace().collect();
        let [old_mode, new_mode, blob, _, status] = words[..] else { return Err("invalid git diff header".into()) };
        let old = fields.next().ok_or("missing git diff path")?;
        let file = if status.starts_with('R') { fields.next().ok_or("missing rename destination")? } else { old };
        if !old_mode.starts_with(":100") {
            continue;
        }
        let before = Items::read(old, &git(root, &["cat-file", "blob", blob])?)?;
        if before.by_name.is_empty() {
            continue;
        }
        let after = if new_mode.starts_with("100") {
            let source = std::fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
            Items::read(file, &source)?
        } else {
            Items::default()
        };
        compare(old, before, file, after, &mut flags);
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    // why: invalid fixture syntax is a test failure.
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn flags_existing_test_and_assertion_syntax() {
        let before = "#[cfg(test)] mod tests { #[test] fn example() { assert_eq!(1,\n2); assert!(true); } }";
        let after = "#[cfg(test)] mod tests { #[test] fn example() { assert_eq!(1, 3); } #[test] fn new() {} }";
        let flags = |new: &str| {
            let mut flags = Vec::new();
            compare(
                "lib.rs",
                Items::read("lib.rs", before).unwrap(),
                "lib.rs",
                Items::read("lib.rs", new).unwrap(),
                &mut flags,
            );
            flags
        };
        assert_eq!(
            flags(after),
            [
                "lib.rs:2 (base): deleted assert! in tests::example",
                "lib.rs:1: modified assert_eq! in tests::example",
                "lib.rs:1: modified test in tests::example",
            ]
        );
        assert!(flags("").contains(&"lib.rs:1 (base): deleted test in tests::example".into()));
        assert!(flags(&before.replace("1,\n2", "1, 2")).is_empty());
        let helper = "#![cfg(test)] fn helper() { wrap!({ assert_eq!(1, 2); }); }";
        let mut flags = Vec::new();
        compare("lib.rs", Items::read("lib.rs", helper).unwrap(), "lib.rs", Items::default(), &mut flags);
        assert_eq!(flags, ["lib.rs:1 (base): deleted assert_eq! in helper"]);
    }
}
