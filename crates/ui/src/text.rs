//! Agent text made safe to draw (5.7.5): control characters and bidirectional overrides go.

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';

/// The text with its control characters (but line breaks and tabs), escape sequences and
/// bidirectional overrides removed; everything else intact.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ESC {
            skip_escape(&mut chars);
        } else if keeps(c) {
            out.push(c);
        }
    }
    out
}

fn keeps(c: char) -> bool {
    if c == '\n' || c == '\t' {
        return true;
    }
    let bidi = matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    !c.is_control() && !bidi
}

fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        }
        Some(']' | 'P' | '_' | '^' | 'X') => {
            while let Some(c) = chars.next() {
                if c == BEL {
                    break;
                }
                if c == ESC {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        _ => {}
    }
}
