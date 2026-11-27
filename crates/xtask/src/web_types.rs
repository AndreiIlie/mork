mod render;
mod source;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use render::Render;
use source::Source;

const OUTPUT: &str = "web/src/types.ts";
const ROOTS: [&str; 4] =
    ["crate::protocol::Request", "crate::protocol::Frame", "crate::chat::HistoryAnswer", "crate::usage::UsageAnswer"];
const COMMAND: &str = "cargo xtask web-types";

fn generate(root: &Path) -> Result<String, String> {
    let source = Source::read(root)?;
    let mut pending = BTreeSet::from(ROOTS.map(str::to_owned));
    let mut seen = BTreeSet::new();
    let mut declarations = BTreeMap::new();
    while let Some(path) = pending.pop_first() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let def = source.definitions.get(&path).ok_or_else(|| format!("crates/types/src/lib.rs: missing {path}"))?;
        let mut render = Render { source: &source, pending: &mut pending, module: &def.module };
        let body = render.declaration(def).map_err(|e| format!("{}:{}: {e}", def.file, e.span().start().line))?;
        let name = render::name(&path).to_owned();
        if declarations.insert(name.clone(), body).is_some() {
            return Err(format!(
                "{}:{}: duplicate TypeScript name {name}",
                def.file,
                def.input.ident.span().start().line
            ));
        }
    }
    let mut out = format!("// Generated from crates/types by {COMMAND}.\n");
    for (name, body) in declarations {
        let gap = if body.starts_with('\n') { "" } else { " " };
        out.push_str(&format!("\nexport type {name} ={gap}{body};\n"));
    }
    Ok(out)
}

pub fn write(root: &Path) -> Result<(), String> {
    let text = generate(root)?;
    fs::write(root.join(OUTPUT), text).map_err(|e| format!("{OUTPUT}: {e}"))
}

pub fn check(root: &Path) -> Result<(), String> {
    let expected = generate(root)?;
    match fs::read(root.join(OUTPUT)) {
        Ok(actual) if actual == expected.as_bytes() => Ok(()),
        Ok(_) => Err(format!("{OUTPUT}:1: generated wire types are stale. Run {COMMAND}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("crates/types/src/lib.rs:1: missing {OUTPUT}. Run {COMMAND}"))
        }
        Err(e) => Err(format!("{OUTPUT}: {e}")),
    }
}
