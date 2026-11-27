use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use syn::{DeriveInput, Item};

pub(super) struct Definition {
    pub file: String,
    pub module: String,
    pub input: DeriveInput,
}

#[derive(Default)]
pub(super) struct Source {
    pub definitions: BTreeMap<String, Definition>,
    imports: HashMap<String, HashMap<String, String>>,
}

impl Source {
    pub fn read(root: &Path) -> Result<Self, String> {
        let mut source = Self::default();
        let dir = Path::new("crates/types/src");
        source.read_file(root, &dir.join("lib.rs"), dir, "crate")?;
        Ok(source)
    }

    fn read_file(&mut self, root: &Path, file: &Path, dir: &Path, module: &str) -> Result<(), String> {
        let path = file.display();
        let text = fs::read_to_string(root.join(file)).map_err(|e| format!("{path}: {e}"))?;
        let parsed = syn::parse_file(&text).map_err(|e| format!("{path}:{}: {e}", e.span().start().line))?;
        self.module(root, file, dir, module, parsed.items)
    }

    fn module(&mut self, root: &Path, file: &Path, dir: &Path, module: &str, items: Vec<Item>) -> Result<(), String> {
        self.imports.insert(module.to_owned(), crate::rust::aliases_of(items.iter()));
        for item in items {
            let input: DeriveInput = match item {
                Item::Struct(s) => s.into(),
                Item::Enum(e) => e.into(),
                Item::Mod(m) => {
                    self.child(root, file, dir, module, m)?;
                    continue;
                }
                _ => continue,
            };
            self.definitions.insert(
                format!("{module}::{}", input.ident),
                Definition { file: file.display().to_string(), module: module.to_owned(), input },
            );
        }
        Ok(())
    }

    fn child(&mut self, root: &Path, file: &Path, dir: &Path, module: &str, m: syn::ItemMod) -> Result<(), String> {
        // References into an unmapped module fail at the wire field that needs it.
        if m.attrs.iter().any(|a| ["path", "cfg", "cfg_attr"].iter().any(|k| a.path().is_ident(k))) {
            return Ok(());
        }
        let name = format!("{module}::{}", m.ident);
        let child = dir.join(m.ident.to_string());
        if let Some((_, items)) = m.content {
            return self.module(root, file, &child, &name, items);
        }
        let sibling = child.with_extension("rs");
        let nested = child.join("mod.rs");
        let path = if root.join(&sibling).is_file() { sibling } else { nested };
        self.read_file(root, &path, &child, &name)
    }

    // Imported names can pass through several re-exports, including the crate root's ID types.
    pub fn resolve(&self, module: &str, path: &syn::Path) -> Result<String, String> {
        let names: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let name = names.join("::");
        let mut full = self.qualify(module, &name);
        let mut seen = HashSet::new();
        while let Some((owner, short)) = full.rsplit_once("::") {
            let Some(import) = self.imports.get(owner).and_then(|imports| imports.get(short)) else { break };
            if !seen.insert(full.clone()) {
                return Err(format!("cyclic import {full}"));
            }
            full = self.qualify(owner, import);
        }
        Ok(full)
    }

    fn qualify(&self, module: &str, path: &str) -> String {
        if let Some(rest) = path.strip_prefix("super::") {
            return self.qualify(module.rsplit_once("::").map_or("crate", |p| p.0), rest);
        }
        let local = format!("{module}::{}", path.trim_start_matches("self::"));
        let head = path.split("::").next().unwrap_or(path);
        if self.definitions.contains_key(&local)
            || self.imports.get(module).is_some_and(|imports| imports.contains_key(head))
            || self.imports.contains_key(&format!("{module}::{head}"))
        {
            return local;
        }
        path.to_owned()
    }
}
