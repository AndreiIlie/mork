//! The one palette (5.2, T5.3): no source file of the window but its theme holds a colour.

use std::path::Path;

use crate::walk;

const COLOURS: [&str; 5] = ["Color::", "Color {", "Color{", "rgb(", "rgba("];

/// Every line of `crates/ui/src` outside `theme.rs` that holds a colour.
pub fn check(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for f in walk::rust_files(&root.join("crates/ui/src")) {
        let rel = walk::rel(root, &f);
        if rel.ends_with("/theme.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if COLOURS.iter().any(|c| code.contains(c)) {
                out.push(format!("{rel}:{}: a colour outside the theme", n + 1));
            }
        }
    }
    out
}
