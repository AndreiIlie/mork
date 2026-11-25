use std::path::Path;

use super::TscError;
use super::lexer::scan_ts;
use super::scan::{scan_block_comment, scan_quoted};
use crate::walk;

const NEG_MARGIN: [&str; 9] = ["-m-", "-mx-", "-my-", "-mt-", "-mr-", "-mb-", "-ml-", "-ms-", "-me-"];
const ROUNDED_SIDES: [&str; 14] = ["t", "r", "b", "l", "s", "e", "tl", "tr", "br", "bl", "ss", "se", "ee", "es"];
const ROUNDED_SIZES: [&str; 11] = ["none", "xs", "sm", "md", "lg", "xl", "2xl", "3xl", "4xl", "full", ""];
const PROSE_WORDS: [&str; 22] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "its", "not", "of", "on", "or",
    "that", "the", "to", "was",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StringPolicy {
    AlwaysClass,
    Heuristic,
    Skip,
}

// Ignore colons inside `[...]` / `(...)` arbitrary values so `hover:rounded-[length:3px]` keeps its utility name.
fn strip_variants(token: &str) -> &str {
    let mut depth: usize = 0;
    let mut last_colon = None;
    for (i, b) in token.bytes().enumerate() {
        match b {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            b':' if depth == 0 => last_colon = Some(i),
            _ => {}
        }
    }
    let tail = last_colon.map_or(token, |i| &token[i + 1..]);
    tail.trim_start_matches('!').trim_end_matches('!')
}

fn is_arbitrary(s: &str) -> bool {
    (s.starts_with('[') && s.ends_with(']')) || (s.starts_with('(') && s.ends_with(')'))
}

fn is_rounded_suffix(rest: &str) -> bool {
    if ROUNDED_SIDES.contains(&rest) {
        return true;
    }
    let size = rest.split_once('-').filter(|(side, _)| ROUNDED_SIDES.contains(side)).map_or(rest, |(_, tail)| tail);
    ROUNDED_SIZES.contains(&size) || is_arbitrary(size)
}

fn is_margin_value(val: &str) -> bool {
    val.is_empty()
        || matches!(val, "px" | "auto" | "full" | "gutter" | "edge")
        || is_arbitrary(val)
        || val.bytes().next().is_some_and(|b| b.is_ascii_digit())
}

fn forbidden_reason(token: &str, allow_bare_rounded: bool) -> Option<&'static str> {
    let base = strip_variants(token);
    if base == "rounded" {
        return (!allow_bare_rounded || token != "rounded").then_some("rounded-corner utility outside web/src/ui/");
    }
    if let Some(rest) = base.strip_prefix("rounded-") {
        return is_rounded_suffix(rest).then_some("rounded-corner utility outside web/src/ui/");
    }
    if let Some(val) = NEG_MARGIN.iter().find_map(|p| base.strip_prefix(p)) {
        return is_margin_value(val).then_some("negative-margin utility outside web/src/ui/");
    }
    None
}

// Outside className bindings, treat bare `"rounded"` as prose if the string has capitals, punctuation, or stop words.
fn looks_like_class_list(text: &str) -> bool {
    for tok in text.split_whitespace() {
        if tok.bytes().next().is_some_and(|b| b.is_ascii_uppercase()) {
            return false;
        }
        if tok.ends_with(['.', ',', ';', '?']) || PROSE_WORDS.contains(&tok) {
            return false;
        }
    }
    true
}

pub(super) fn check_segment(
    rel: &str,
    lines: &[&str],
    start_line: u64,
    text: &str,
    policy: StringPolicy,
    out: &mut Vec<TscError>,
) {
    if policy == StringPolicy::Skip {
        return;
    }
    let allow_bare = policy == StringPolicy::Heuristic && !looks_like_class_list(text);
    for (offset, chunk) in text.split('\n').enumerate() {
        let line = start_line + offset as u64;
        let raw = lines.get((line - 1) as usize).copied().unwrap_or(chunk).trim();
        for token in chunk.split_whitespace() {
            if let Some(reason) = forbidden_reason(token, allow_bare) {
                out.push(TscError {
                    file: rel.to_owned(),
                    line,
                    summary: format!("forbidden class `{token}`: {reason}"),
                    proof: format!("web/{rel}:{line}: {token}\n  {raw}"),
                });
            }
        }
    }
}

fn scan_css(rel: &str, text: &str) -> Vec<TscError> {
    let bytes = text.as_bytes();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line: u64 = 1;
    let mut in_apply = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let (next, nl) = scan_block_comment(bytes, i);
                i = next;
                line += nl;
            }
            q @ (b'"' | b'\'') => i = scan_quoted(bytes, i, q, false).map_or(i + 1, |(end, _)| end + 1),
            b'@' if text[i..].starts_with("@apply") && bytes.get(i + 6).is_some_and(u8::is_ascii_whitespace) => {
                in_apply = true;
                i += 6;
            }
            b';' | b'}' if in_apply => {
                in_apply = false;
                i += 1;
            }
            _ if in_apply && !bytes[i].is_ascii_whitespace() => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b';' | b'}') {
                    i += 1;
                }
                check_segment(rel, &lines, line, &text[start..i], StringPolicy::AlwaysClass, &mut out);
            }
            _ => i += 1,
        }
    }
    out
}

/// Scans one `web/`-relative source file for forbidden rounded and negative-margin utilities.
pub fn scan_classes(rel_to_web: &str, text: &str) -> Vec<TscError> {
    if rel_to_web.starts_with("src/ui/") {
        return Vec::new();
    }
    if rel_to_web.ends_with(".css") {
        return scan_css(rel_to_web, text);
    }
    scan_ts(rel_to_web, text)
}

/// Scans `web/src/**/*.{ts,tsx,css}` outside `web/src/ui/` for forbidden rounded and negative-margin classes.
pub fn check_classes(root: &Path) -> Vec<TscError> {
    let web = root.join("web");
    let src = web.join("src");
    let mut files = Vec::new();
    for ext in ["ts", "tsx", "css"] {
        files.extend(walk::files(&src, ext));
    }
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let rel = walk::rel(&web, &f).replace('\\', "/");
        if let Ok(text) = std::fs::read_to_string(&f) {
            out.extend(scan_classes(&rel, &text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(rel: &str, src: &str) -> Vec<(u64, String)> {
        scan_classes(rel, src)
            .into_iter()
            .map(|e| (e.line, e.summary.split('`').nth(1).unwrap_or("").to_owned()))
            .collect()
    }

    #[test]
    fn refuses_rounded_and_negative_margin_classes_outside_ui() {
        let source = "\
// comments mentioning rounded-full or -mx-3 are ignored
/* block comment with lg:-mt-2 */
const ok = \"mx-3 border px-edge\";
const a = \"h-2 w-2 rounded-full\";
const b = \"-mx-3 mt-3 divide-y\";
const c = \"lg:-mt-2 hover:rounded\";
const d = \"hover:rounded-[length:3px] -mx-[length:3px]\";
const e = `flex ${ok ? \"-my-2\" : \"\"} lg:rounded-sm`;
";
        assert_eq!(
            found("src/Board.tsx", source),
            vec![
                (4, "rounded-full".into()),
                (5, "-mx-3".into()),
                (6, "lg:-mt-2".into()),
                (6, "hover:rounded".into()),
                (7, "hover:rounded-[length:3px]".into()),
                (7, "-mx-[length:3px]".into()),
                (8, "-my-2".into()),
                (8, "lg:rounded-sm".into()),
            ]
        );
        assert!(scan_classes("src/ui/Dot.tsx", source).is_empty());
    }

    #[test]
    fn handles_comment_markers_in_strings_and_ignores_prose_and_non_class_attrs() {
        let source = "\
export const Upload = () => <input type=\"file\" accept=\"image/*\" className=\"-mx-3\" />;
export const Marker = () => <div className=\"rounded-full\" />;
export const Separator = () => <div className=\"before:content-['//'] -my-2\" />;
const rounded = Math.round(2.5);
export const Total = () => <p title=\"Durations are rounded to whole minutes\">{\"Totals are rounded to the nearest dollar.\"} {rounded}</p>;
export const Text = () => <div className=\"before:content-['rounded-full']\" />;
const re = /[.,;:!?'\"”’]$/;
const prose = \"Values are rounded to whole minutes.\";
export const AfterRe = () => <div className={prose ? \"rounded\" : \"-ml-2\"} />;
";
        assert_eq!(
            found("src/ClassProbe.tsx", source),
            vec![
                (1, "-mx-3".into()),
                (2, "rounded-full".into()),
                (3, "-my-2".into()),
                (9, "rounded".into()),
                (9, "-ml-2".into()),
            ]
        );
    }

    #[test]
    fn catches_multiline_jsx_classes_and_tags_after_closing_tags() {
        let source = "\
export const Wrapped = () => (
  <div className=\"
    flex rounded-[13px]
    -mx-[17px]
  \" />
);
export const Inline = () => <section><p>Totals</p><div className=\"rounded-[19px] -mx-[23px]\" /></section>;
";
        assert_eq!(
            found("src/ClassProbe.tsx", source),
            vec![
                (3, "rounded-[13px]".into()),
                (4, "-mx-[17px]".into()),
                (7, "rounded-[19px]".into()),
                (7, "-mx-[23px]".into()),
            ]
        );
    }

    #[test]
    fn handles_comments_before_jsx_callback_vars_and_short_class_constants() {
        let copy = "\
export function PrecisionBadge() {
  return (
    // Keep the caption short.
    <span title=\"rounded\">{\"rounded\"}</span>
  );
}
const HINT = \"Displays a rounded-down percentage.\";
export const Hint = () => <span title={HINT}>%</span>;
";
        assert!(found("src/Copy.tsx", copy).is_empty());

        let callback = "\
export function Steps({ steps }: { steps: string[] }) {
  return (
    <ol>
      {steps.map((s) => {
        const tone = s === \"now\" ? \"font-medium rounded-[13px] -mx-[17px]\" : \"text-gray-700\";
        return <li key={s} className={tone}>{s}</li>;
      })}
    </ol>
  );
}
const ROW = \"flex rounded\";
export const List = () => <div className={ROW}>Rows</div>;
";
        assert_eq!(
            found("src/Steps.tsx", callback),
            vec![(5, "rounded-[13px]".into()), (5, "-mx-[17px]".into()), (11, "rounded".into())]
        );
    }

    #[test]
    fn scans_css_apply_rules_and_skips_css_comments() {
        let css = "\
/* @apply rounded-full -mx-3; */
.card {
  content: \"/*\";
  @apply hover:rounded-[length:3px] -mx-3;
}
";
        assert_eq!(found("src/extra.css", css), vec![(4, "hover:rounded-[length:3px]".into()), (4, "-mx-3".into())]);
    }
}
