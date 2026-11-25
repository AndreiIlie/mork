//! Andrei's access file (fac-86): only the engine reads it; nothing in it is followed or run. Its
//! errors never name a value (agents see them), and every secret-looking value is masked in door output.

use std::path::Path;

use factory_types::door_plan::Pair;

const MAX: u64 = 1024 * 1024;

#[derive(Default)]
pub struct AccessFile {
    sections: Vec<Section>,
}

struct Section {
    name: String,
    line: usize,
    fields: Vec<(String, String, usize)>,
}

/// Reads it: a regular file, readable by its owner alone.
pub fn read(path: &Path) -> Result<AccessFile, String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| format!("the access file can't be read: {}", e.kind()))?;
    if !meta.is_file() {
        return Err("the access file isn't a regular file".into());
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!("the access file is readable by others (mode {mode:o}): it is to be 600"));
    }
    if meta.len() > MAX {
        return Err("the access file is over 1 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("the access file can't be read: {}", e.kind()))?;
    let text = String::from_utf8(bytes).map_err(|_| "the access file isn't UTF-8 text".to_owned())?;
    Ok(parse(&text))
}

/// Parses its text. A section runs from its `[access:<name>]` line to the next section, heading
/// (`#`) or other bracketed line; in it, a `key = value` line is a field, anything else prose.
pub fn parse(text: &str) -> AccessFile {
    let mut f = AccessFile::default();
    let mut open = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix("[access:").and_then(|r| r.strip_suffix(']')) {
            f.sections.push(Section { name: name.trim().to_owned(), line: i + 1, fields: Vec::new() });
            open = true;
        } else if line.starts_with('#') || line.starts_with('[') {
            open = false;
        } else if open
            && let Some((k, v)) = field(line)
            && let Some(s) = f.sections.last_mut()
        {
            s.fields.push((k.to_owned(), v.to_owned(), i + 1));
        }
    }
    f
}

fn field(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    let k = k.trim();
    let named = !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    named.then_some((k, v.trim()))
}

fn lines(at: &[usize]) -> String {
    at.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
}

impl AccessFile {
    /// Two fields of one section, each present once: a non-empty name and a secret, which may be
    /// empty only when `allow_empty_secret` is true.
    pub fn pair(&self, section: &str, name: &str, secret: &str, allow_empty_secret: bool) -> Result<Pair, String> {
        let found: Vec<&Section> = self.sections.iter().filter(|s| s.name == section).collect();
        let s = match found.as_slice() {
            [] => return Err(format!("the access file has no section [access:{section}]")),
            [one] => *one,
            many => {
                let at: Vec<usize> = many.iter().map(|s| s.line).collect();
                return Err(format!("the access file has [access:{section}] more than once (lines {})", lines(&at)));
            }
        };
        Ok(Pair { name: s.value(name, false)?, secret: s.value(secret, allow_empty_secret)? })
    }

    /// Secret-looking field values, except `auth_database`, `credentials_file`, `credentials_key` and
    /// `credentials_format`, plus every URI's password, in every section: what door output masks.
    pub fn secrets(&self) -> Vec<String> {
        let all = self.sections.iter().flat_map(|s| s.fields.iter());
        let named =
            all.clone().filter(|(k, v, _)| !v.is_empty() && super::redact::secret_key(k)).map(|(_, v, _)| v.clone());
        named.chain(all.filter_map(|(_, v, _)| super::redact::uri_password(v))).collect()
    }
}

impl Section {
    fn value(&self, key: &str, allow_empty: bool) -> Result<String, String> {
        let found: Vec<&(String, String, usize)> = self.fields.iter().filter(|(k, _, _)| k == key).collect();
        let at: Vec<usize> = found.iter().map(|(_, _, l)| *l).collect();
        match found.as_slice() {
            [] => Err(format!("the access file's [access:{}] has no `{key}`", self.name)),
            [(_, v, _)] if v.is_empty() && !allow_empty => {
                Err(format!("the access file's [access:{}] `{key}` is empty", self.name))
            }
            [(_, v, _)] => Ok(v.clone()),
            _ => Err(format!(
                "the access file's [access:{}] has `{key}` more than once (lines {})",
                self.name,
                lines(&at)
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    // why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
    #![allow(clippy::unwrap_used)]

    use super::*;

    // fac-86: the file is read only when it is its owner's alone.
    #[test]
    fn fac86_an_access_file_others_can_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("factory-test-access-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ACCESS.md");
        std::fs::write(&path, "[access:x]\nuser = a\npassword = SENTINEL-p\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let e = read(&path).err().unwrap();
        assert!(e.contains("mode 644") && !e.contains("SENTINEL"), "{e}");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read(&path).unwrap().pair("x", "user", "password", false).unwrap().secret, "SENTINEL-p");
        assert!(read(&dir.join("gone")).err().unwrap().contains("can't be read"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
