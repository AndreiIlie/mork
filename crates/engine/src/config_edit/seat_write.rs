use mork_types::config::view::SeatDefault;
use mork_types::config::{Config, PickModel, SEAT_ROWS, Standing};
use toml_edit::Value;

use super::document::{self, Result};

fn row(key: &str) -> Result<(&'static str, Standing)> {
    let (name, part) = key.split_once('.').ok_or("Unknown seat standing.")?;
    SEAT_ROWS
        .iter()
        .copied()
        .find(|&(s, at)| s == name && at.part() == part)
        .ok_or_else(|| "Unknown seat standing.".into())
}

fn sibling(name: &str, part: &str) -> Option<&'static str> {
    SEAT_ROWS.iter().find(|&&(s, at)| s == name && at.part() != part).map(|(_, at)| at.part())
}

fn reset(s: &mut String, name: &str, part: &str) -> Result<()> {
    for key in ["cli", "model", "effort"] {
        let common = ["seats", name, key];
        let parsed = document::doc(s)?;
        let value = document::item(parsed.as_item(), &common).and_then(|v| v.as_str()).map(str::to_owned);
        if let (Some(other), Some(value)) = (sibling(name, part), value) {
            let path = ["seats", name, other, key];
            if document::item(parsed.as_item(), &path).is_none() {
                document::set(s, &path, Some(value.into()))?;
            }
        }
        document::set(s, &common, None)?;
        document::set(s, &["seats", name, part, key], None)?;
    }
    document::prune(s, &["seats", name, part])?;
    document::prune(s, &["seats", name])
}

fn assign(s: &mut String, name: &str, part: &str, key: &str, value: &str) -> Result<()> {
    let path = ["seats", name, part, key];
    let use_part = sibling(name, part).is_some() || document::item(document::doc(s)?.as_item(), &path).is_some();
    if use_part {
        document::set(s, &path, Some(Value::from(value)))
    } else {
        document::set(s, &["seats", name, key], Some(Value::from(value)))
    }
}

pub(super) fn write(
    s: &mut String,
    key: &str,
    old: &SeatDefault,
    next: &SeatDefault,
    fallback: &SeatDefault,
    offers: &[PickModel],
) -> Result<()> {
    let (name, at) = row(key)?;
    let part = at.part();
    let is_default = next.cli == fallback.cli && next.model == fallback.model && next.effort == fallback.effort;
    if !next.configured && !is_default {
        return Err("Use the seat’s default model and effort.".into());
    }
    if is_default {
        return reset(s, name, part);
    }
    if !offers.iter().any(|m| m.cli == next.cli && m.model == next.model && m.efforts.contains(&next.effort)) {
        return Err("This model and effort are unavailable for the seat.".into());
    }
    let model_changed = old.cli != next.cli || old.model != next.model;
    if model_changed {
        assign(s, name, part, "cli", next.cli.as_str())?;
        assign(s, name, part, "model", &next.model)?;
    }
    if old.effort != next.effort || model_changed && !effective_effort(s, name, at, next)? {
        assign(s, name, part, "effort", next.effort.as_str())?;
    }
    Ok(())
}

fn effective_effort(text: &str, name: &str, at: Standing, wanted: &SeatDefault) -> Result<bool> {
    let config: Config = toml::from_str(text).map_err(|_| "Could not read the seat override.")?;
    Ok(config.seat_in(name, at).is_some_and(|s| s.effort == wanted.effort))
}

pub(super) fn verify(c: &Config, key: &str, wanted: &SeatDefault) -> Result<()> {
    let (name, at) = row(key)?;
    let s = c.seat_in(name, at).ok_or("Missing seat.")?;
    if (s.cli, &s.model, s.effort) != (wanted.cli, &wanted.model, wanted.effort) {
        return Err("The seat override does not produce the requested model and effort.".into());
    }
    Ok(())
}
