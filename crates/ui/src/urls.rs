use crate::markdown::Run;

pub(crate) fn runs(mut run: Run) -> Vec<Run> {
    let chars: Vec<char> = std::mem::take(&mut run.text).chars().collect();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < chars.len() {
        if let Some(n) = length(&chars[i..]) {
            if start < i {
                out.push(Run { text: chars[start..i].iter().collect(), ..run.clone() });
            }
            out.push(Run { text: chars[i..i + n].iter().collect(), link: true, ..run.clone() });
            i += n;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < chars.len() {
        out.push(Run { text: chars[start..].iter().collect(), ..run });
    }
    out
}

pub(crate) fn length(chars: &[char]) -> Option<usize> {
    if !chars.starts_with(&['h', 't', 't', 'p', ':', '/', '/'])
        && !chars.starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/'])
    {
        return None;
    }
    let mut end = chars.iter().position(|c| c.is_whitespace() || c.is_control()).unwrap_or(chars.len());
    let balance = |end, open, close| {
        chars[..end].iter().filter(|c| **c == close).count() > chars[..end].iter().filter(|c| **c == open).count()
    };
    while end > 0 {
        let trim = match chars[end - 1] {
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '`' | '“' | '”' | '‘' | '’' => true,
            ')' => balance(end, '(', ')'),
            ']' => balance(end, '[', ']'),
            '>' => balance(end, '<', '>'),
            '}' => balance(end, '{', '}'),
            _ => false,
        };
        if !trim {
            break;
        }
        end -= 1;
    }
    Some(end)
}
