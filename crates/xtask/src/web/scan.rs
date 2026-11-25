pub(super) fn skip_escape(bytes: &[u8], i: usize) -> usize {
    if bytes.get(i + 1).is_some_and(|b| *b < 0x80) { 2 } else { 1 }
}

pub(super) fn scan_block_comment(bytes: &[u8], start: usize) -> (usize, u64) {
    let mut i = start + 2;
    let mut newlines = 0;
    while i < bytes.len() {
        if bytes.get(i..i + 2) == Some(b"*/") {
            return (i + 2, newlines);
        }
        if bytes[i] == b'\n' {
            newlines += 1;
        }
        i += 1;
    }
    (bytes.len(), newlines)
}

pub(super) fn scan_quoted(bytes: &[u8], start: usize, quote: u8, multiline: bool) -> Option<(usize, u64)> {
    let mut i = start + 1;
    let mut newlines = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if bytes.get(i + 1) == Some(&b'\n') {
                    newlines += 1;
                }
                i += skip_escape(bytes, i);
            }
            b'\n' if !multiline => return None,
            b'\n' => {
                newlines += 1;
                i += 1;
            }
            b if b == quote => return Some((i, newlines)),
            _ => i += 1,
        }
    }
    None
}

pub(super) fn scan_regex(bytes: &[u8], start: usize) -> Option<usize> {
    if matches!(bytes.get(start + 1), None | Some(b'/' | b'*' | b'\n')) {
        return None;
    }
    let mut i = start + 1;
    let mut in_class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += skip_escape(bytes, i),
            b'\n' => return None,
            b'[' => {
                in_class = true;
                i += 1;
            }
            b']' => {
                in_class = false;
                i += 1;
            }
            b'/' if !in_class => return Some(i),
            _ => i += 1,
        }
    }
    None
}

pub(super) fn is_class_attr(name: &str) -> bool {
    name == "className" || name == "class" || name.ends_with("ClassName") || name.ends_with("Class")
}

fn skip_generic(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => depth += 1,
            b'>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b'\n' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

pub(super) fn scan_tag_head(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    if !bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-' | b'.' | b':')) {
        i += 1;
    }
    if bytes.get(i) == Some(&b'<') {
        i = skip_generic(bytes, i)?;
    }
    bytes.get(i).is_some_and(|b| matches!(b, b'>' | b'/') || b.is_ascii_whitespace()).then_some(i)
}

pub(super) fn is_expr_punct(b: u8) -> bool {
    matches!(
        b,
        b'=' | b'('
            | b'['
            | b'{'
            | b','
            | b';'
            | b':'
            | b'!'
            | b'?'
            | b'&'
            | b'|'
            | b'^'
            | b'~'
            | b'+'
            | b'-'
            | b'*'
            | b'%'
    )
}

pub(super) fn next_is_condition_op(bytes: &[u8], mut i: usize) -> bool {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    matches!(bytes.get(i), Some(b'?')) || matches!(bytes.get(i..i + 2), Some(b"&&" | b"==" | b"!=" | b"<=" | b">="))
}
