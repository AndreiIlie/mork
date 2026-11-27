//! Usage limits: Claude's `rate_limit_event` windows, Codex's limits answer (F-X5) and agy's `/usage`, one parser
//! each (1.12.3).

use serde_json::Value;

/// One window of a usage limit, as a CLI reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub account: Option<mork_types::usage::Account>,
    pub seen_ms: Option<i64>,
    /// The account's own (`account`), or a model's or a limit's own.
    pub bucket: String,
    /// `5h`, `week`, `<n>m`, or Claude's own names (`five_hour`, `seven_day`, …), kept as
    /// reported; [`mork_types::usage::short_name`] is how they show.
    pub window: String,
    pub percent: Option<u32>,
    pub resets_at: Option<i64>,
    pub reached: bool,
}

pub const ACCOUNT: &str = "account";

/// The account's organization and plan from a Claude configuration, when both are known.
pub fn claude_account(config: &Value) -> Option<mork_types::usage::Account> {
    let a = config.get("oauthAccount")?;
    let id = a.get("accountUuid")?.as_str().filter(|s| !s.is_empty())?;
    let org = a.get("organizationUuid")?.as_str().filter(|s| !s.is_empty())?;
    let plan = match a.get("organizationType").and_then(Value::as_str) {
        Some("claude_team") => "team",
        Some("claude_max") => "max",
        _ if a.get("userRateLimitTier").and_then(Value::as_str).is_some_and(|t| t.contains("claude_max")) => "max",
        _ => return None,
    };
    Some(mork_types::usage::Account { id: format!("{org}/{id}"), plan: plan.into() })
}

fn percent(fraction: Option<&Value>) -> Option<u32> {
    let f = fraction?.as_f64()?;
    if !f.is_finite() || f < 0.0 {
        return None;
    }
    let p = (f * 100.0).round();
    // why: a finite, non-negative share below u32's range converts exactly after rounding.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (p < f64::from(u32::MAX)).then_some(p as u32)
}

/// Claude's `rate_limit_info`: the named window carries its status, including when absent from `unifiedWindows`.
pub fn claude_windows(info: &Value) -> Vec<Window> {
    let named = info.get("rateLimitType").and_then(Value::as_str);
    let rejected = info.get("status").and_then(Value::as_str) == Some("rejected");
    let mut out = Vec::new();
    if let Some(ws) = info.get("unifiedWindows").and_then(Value::as_object) {
        for (name, w) in ws {
            out.push(Window {
                account: None,
                seen_ms: None,
                bucket: ACCOUNT.into(),
                window: name.clone(),
                percent: percent(w.get("utilization")),
                resets_at: w.get("resetsAt").and_then(Value::as_i64),
                reached: rejected && Some(name.as_str()) == named,
            });
        }
    }
    if let Some(n) = named.filter(|n| !out.iter().any(|w| w.window == *n)) {
        out.push(Window {
            account: None,
            seen_ms: None,
            bucket: match n {
                "five_hour" | "seven_day" | "seven_day_overage_included" => ACCOUNT.to_owned(),
                _ => n.to_owned(),
            },
            window: n.to_owned(),
            percent: percent(info.get("utilization")),
            resets_at: info.get("resetsAt").and_then(Value::as_i64),
            reached: rejected,
        });
    }
    out
}

// By its length, never by its slot.
fn codex_window_name(mins: i64) -> String {
    match mins {
        300 => "5h".into(),
        10_080 => "week".into(),
        n => format!("{n}m"),
    }
}

fn codex_entry(bucket: &str, e: &Value) -> Vec<Window> {
    let reached_type = e.get("rateLimitReachedType").filter(|v| !v.is_null());
    let named_slot = reached_type.and_then(Value::as_str).filter(|t| ["primary", "secondary"].contains(t));
    let reached = |slot: &str| match (reached_type, named_slot) {
        (None, _) => false,
        (Some(_), Some(n)) => n == slot,
        (Some(_), None) => true,
    };
    ["primary", "secondary"]
        .iter()
        .filter_map(|slot| {
            let w = e.get(*slot).filter(|w| w.is_object())?;
            let mins = w.get("windowDurationMins").and_then(Value::as_i64)?;
            Some(Window {
                account: None,
                seen_ms: None,
                bucket: bucket.to_owned(),
                window: codex_window_name(mins),
                percent: w
                    .get("usedPercent")
                    .and_then(Value::as_f64)
                    .and_then(|p| percent(Some(&Value::from(p / 100.0)))),
                resets_at: w.get("resetsAt").and_then(Value::as_i64),
                reached: reached(slot),
            })
        })
        .collect()
}

/// `codex app-server`'s id 2 answer (F-X5, 1.4.8, Codex 7): entries other than `codex` or tied to a model
/// are buckets of their own; the account's session and week are always there, unknown when not reported.
pub fn codex_read(text: &str) -> Result<Vec<Window>, String> {
    let answer = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("id").and_then(Value::as_u64) == Some(2))
        .ok_or("no answer to the limits read")?;
    if let Some(e) = answer.get("error") {
        let m = e.get("message").and_then(Value::as_str).unwrap_or("an error");
        return Err(format!("the limits read failed: {m}"));
    }
    let r = answer.get("result").ok_or("the limits answer has no result")?;
    let mut windows = Vec::new();
    let by_id = r.get("rateLimitsByLimitId").and_then(Value::as_object);
    let entries: Vec<(String, &Value)> = match by_id {
        Some(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        None => r.get("rateLimits").map(|v| vec![("codex".to_owned(), v)]).unwrap_or_default(),
    };
    for (id, e) in &entries {
        let model = e.get("normalModelSlug").and_then(Value::as_str);
        let bucket = match (id.as_str(), model) {
            ("codex", None) => ACCOUNT.to_owned(),
            (_, Some(m)) => m.to_owned(),
            (other, None) => other.to_owned(),
        };
        windows.extend(codex_entry(&bucket, e));
    }
    for name in ["5h", "week"] {
        if !windows.iter().any(|w| w.bucket == ACCOUNT && w.window == name) {
            windows.push(Window {
                account: None,
                seen_ms: None,
                bucket: ACCOUNT.into(),
                window: name.into(),
                percent: None,
                resets_at: None,
                reached: false,
            });
        }
    }
    let account_out = r.get("ordinaryUsageAllowed").and_then(Value::as_bool) == Some(false);
    if account_out {
        for w in windows.iter_mut().filter(|w| w.bucket == ACCOUNT) {
            w.reached = true;
        }
    }
    Ok(windows)
}

/// agy's `/usage` answer (fac-657, fac-661): `Gemini Models` (`account`) and `Claude and GPT models` (`3p`), each with
/// `5h` and `week`, unknown when absent; a window is reached with nothing left.
pub fn agy_read(text: &str) -> Result<Vec<Window>, String> {
    let start = text.find('{').ok_or("no answer to agy's limits read")?;
    let answer = serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
        .ok_or("agy's limits answer isn't JSON")?;
    let groups =
        answer.pointer("/command/data/groups").and_then(Value::as_array).ok_or("agy's limits answer has no groups")?;
    let find = |name: &str| {
        groups.iter().find(|g| g.as_object().is_some_and(|o| o.values().any(|v| v.as_str() == Some(name))))
    };
    let gemini = find("Gemini Models").ok_or("agy's limits answer has no Gemini Models group")?;
    let mut windows = agy_group(ACCOUNT, Some(gemini));
    windows.extend(agy_group(mork_types::config::AGY_3P_BUCKET, find("Claude and GPT models")));
    Ok(windows)
}

fn agy_group(bucket: &str, group: Option<&Value>) -> Vec<Window> {
    let buckets = group.and_then(|g| g.get("buckets")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    [("5h", "5h"), ("weekly", "week")]
        .iter()
        .map(|(reported, window)| {
            let b = buckets.iter().find(|b| b.get("window").and_then(Value::as_str) == Some(reported));
            let left = b.and_then(|b| b.get("remaining_fraction")).and_then(Value::as_f64).filter(|f| f.is_finite());
            Window {
                account: None,
                seen_ms: None,
                bucket: bucket.into(),
                window: (*window).into(),
                percent: left.and_then(|f| percent(Some(&Value::from((1.0 - f).clamp(0.0, 1.0))))),
                resets_at: b.and_then(|b| b.get("reset_time")).and_then(Value::as_str).and_then(rfc3339_secs),
                reached: left.is_some_and(|f| f <= 0.0),
            }
        })
        .collect()
}

/// An RFC 3339 time (`2026-11-22T17:04:05.123Z`, or with an offset) in epoch seconds.
pub fn rfc3339_secs(t: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, s) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let seps = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    let bytes = t.as_bytes();
    if !seps.iter().all(|(i, c)| bytes.get(*i) == Some(c)) || !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let zone = t.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match zone.as_bytes().first() {
        Some(b'Z' | b'z') if zone.len() == 1 => 0,
        Some(sign @ (b'+' | b'-')) if zone.len() == 6 && zone.as_bytes()[3] == b':' => {
            let m = zone.get(1..3)?.parse::<i64>().ok()? * 60 + zone.get(4..6)?.parse::<i64>().ok()?;
            if *sign == b'+' { m * 60 } else { -m * 60 }
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s - offset)
}
