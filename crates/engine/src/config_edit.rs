mod arrays;
mod document;
mod picks;
mod seat_write;
mod seats;
#[cfg(test)]
mod tests;
mod view;

use crate::{config, files, secrets::Secrets};
use document::Result;
use mork_types::config::view::{ConfigChange, ConfigField, ConfigSection, ConfigValue, ConfigView, FieldChange};
use mork_types::config::{Config, Ports};
use std::fs;
use std::path::Path;
use toml_edit::Value;

pub(crate) const STALE: &str = "mork.toml changed on disk since Mork started. Reload Mork to edit it here.";

pub(crate) fn read(c: &Config, secrets: &Secrets, stale: bool) -> ConfigView {
    view::build(c, secrets, stale)
}

fn locate<'a>(v: &'a ConfigView, change: &ConfigChange) -> Result<&'a ConfigSection> {
    v.sections
        .iter()
        .find(|s| s.section == change.section && s.project == change.project)
        .ok_or_else(|| "Unknown settings section.".into())
}

fn compared<'a>(section: &'a ConfigSection, key: &str, update: &FieldChange) -> Result<&'a ConfigField> {
    let field = section.fields.iter().find(|f| f.key == key).ok_or("Unknown setting.")?;
    let current = field.value.as_ref().ok_or("Withheld: it holds a secret Mork knows. Edit it in mork.toml.")?;
    if current != &update.read {
        let current = match current {
            ConfigValue::Text(s) => s.clone(),
            other => serde_json::to_string(other).map_err(|_| "Could not read setting.")?,
        };
        return Err(format!(
            "{key} changed to {current} in another window since this page read it. Your edit is kept: check it and save again."
        ));
    }
    Ok(field)
}

fn array(key: &str, list: &[String], global: bool) -> Result<Value> {
    let mut array = toml_edit::Array::new();
    for raw in list {
        if key == "domains" {
            array.push(raw.as_str());
            continue;
        }
        let port = Ports::parse(raw)?;
        if port.from == port.to {
            array.push(i64::from(port.from));
        } else if global {
            return Err("Global ports must be individual port numbers.".into());
        } else {
            array.push(format!("{}-{}", port.from, port.to));
        }
    }
    Ok(Value::Array(array))
}

fn toml_value(key: &str, value: &ConfigValue, global: bool) -> Result<Value> {
    match value {
        ConfigValue::Entries(items) if matches!(key, "ports" | "host_ports" | "domains") => array(key, items, global),
        ConfigValue::Text(s) if matches!(key, "mode" | "command" | "folder" | "colleague_review" | "memory_high") => {
            Ok(s.as_str().into())
        }
        ConfigValue::Text(s) => s.parse::<i64>().map(Value::from).map_err(|_| format!("{key} must be a whole number.")),
        _ => Err("Wrong value for this setting.".into()),
    }
}

fn ordinary(s: &mut String, section: &ConfigSection, field: &ConfigField, v: &ConfigValue) -> Result<()> {
    let mut path = match section.project.as_deref() {
        Some(name) => vec!["projects", name],
        None if section.section == "slice" => vec!["slice"],
        None => vec!["mork"],
    };
    path.extend(field.key.split('.'));
    let key = path.last().copied().ok_or("Unknown setting.")?;
    let default = field.default.as_ref() == Some(v);
    let value = if default { None } else { Some(toml_value(key, v, section.project.is_none())?) };
    document::set(s, &path, value)?;
    // Removing its last dotted key must still leave the required mork table.
    if path[0] == "mork" && document::doc(s)?.as_table().get("mork").is_none() {
        document::set(s, &["mork"], Some(Value::InlineTable(Default::default())))?;
    }
    Ok(())
}

fn apply(s: &mut String, section: &ConfigSection, field: &ConfigField, edit: &FieldChange) -> Result<()> {
    if std::mem::discriminant(&edit.read) != std::mem::discriminant(&edit.value) {
        return Err("Wrong value for this setting.".into());
    }
    match (&edit.read, &edit.value, &field.default) {
        (ConfigValue::Seat(old), ConfigValue::Seat(new), Some(ConfigValue::Seat(default))) => {
            seat_write::write(s, &field.key, old, new, default, &field.offers)
        }
        (ConfigValue::Picks(old), ConfigValue::Picks(new), _) => picks::write(s, &field.key, old, new, &field.offers),
        (ConfigValue::Seat(_), _, _) => Err("The seat default is withheld.".into()),
        _ => ordinary(s, section, field, &edit.value),
    }
}

fn candidate(text: &str, cfg: &Config, change: &ConfigChange, secrets: &Secrets) -> Result<(String, Config)> {
    let view = read(cfg, secrets, false);
    let section = locate(&view, change)?;
    let mut text = text.to_owned();
    for (key, edit) in &change.fields {
        let field = compared(section, key, edit)?;
        if edit.value != edit.read {
            apply(&mut text, section, field, edit)?;
        }
    }
    if let Some(project) = &change.project
        && change.fields.keys().any(|key| key.starts_with("checks."))
    {
        document::prune(&mut text, &["projects", project, "checks"])?;
    }
    let cfg = config::parse(&text).map_err(|e| e.to_string())?;
    for (key, edit) in &change.fields {
        if let ConfigValue::Seat(seat) = &edit.value {
            seat_write::verify(&cfg, key, seat)?;
        }
    }
    Ok((text, cfg))
}

pub(crate) fn save(
    path: &Path,
    loaded: &str,
    cfg: &Config,
    change: &ConfigChange,
    secrets: &Secrets,
) -> Result<(String, Config)> {
    let real = path.canonicalize().map_err(|_| STALE)?;
    let folder = fs::File::open(real.parent().ok_or("Missing config directory.")?)
        .map_err(|_| "Cannot open the config directory.")?;
    crate::init::lock(&folder)?;
    let current = fs::read_to_string(&real).map_err(|_| STALE)?;
    if current != loaded {
        return Err(STALE.into());
    }
    let (text, config) = candidate(&current, cfg, change, secrets).map_err(|e| secrets.mask(&e))?;
    if text != current {
        let permissions = real.metadata().map_err(|_| "Cannot read mork.toml permissions.")?.permissions();
        files::write_whole_preserving(&real, text.as_bytes(), permissions).map_err(|_| "Could not write mork.toml.")?;
    }
    Ok((text, config))
}
