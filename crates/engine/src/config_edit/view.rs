use mork_types::config::view::{ConfigField, ConfigSection, ConfigValue, ConfigView};
use mork_types::config::{self, ColleagueReview, Config, ProjectConfig, SliceConfig};
use mork_types::lookback::{DEFAULT_DREAM_BUDGET, Read, Trigger};

use crate::secrets::Secrets;

fn field(key: &str, value: ConfigValue, default: ConfigValue) -> ConfigField {
    ConfigField { key: key.into(), value: Some(value), default: Some(default), offers: Vec::new() }
}

fn text(key: &str, value: impl ToString, default: impl ToString) -> ConfigField {
    field(key, ConfigValue::Text(value.to_string()), ConfigValue::Text(default.to_string()))
}

fn entries(key: &str, values: &[String], default: &[String]) -> ConfigField {
    field(key, ConfigValue::Entries(values.to_vec()), ConfigValue::Entries(default.to_vec()))
}

fn section(name: &str, fields: Vec<ConfigField>) -> ConfigSection {
    ConfigSection { section: name.into(), fields, project: None }
}

fn system(c: &Config) -> [ConfigSection; 3] {
    let m = &c.mork;
    let s = &c.slice;
    let defaults = SliceConfig::default();
    [
        section(
            "limits",
            vec![
                text("max_test_runs", m.max_test_runs, config::default_max_test_runs()),
                text("context_cap", m.context_cap, mork_types::config::CONTEXT_CAP_DEFAULT),
                text("fresh_context_percent", m.fresh_context_percent, config::default_fresh_context_percent()),
            ],
        ),
        section(
            "slice",
            vec![
                text("cpu_weight", s.cpu_weight, defaults.cpu_weight),
                text("io_weight", s.io_weight, defaults.io_weight),
                text("memory_high", &s.memory_high, defaults.memory_high),
            ],
        ),
        section(
            "reach",
            vec![
                entries("domains", &m.domains, &[]),
                entries(
                    "ports",
                    &m.ports.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    &config::default_ports().iter().map(ToString::to_string).collect::<Vec<_>>(),
                ),
            ],
        ),
    ]
}

fn ports(key: &str, ports: &[mork_types::config::Ports]) -> ConfigField {
    let values = ports
        .iter()
        .map(|p| if p.from == p.to { p.from.to_string() } else { format!("{}-{}", p.from, p.to) })
        .collect::<Vec<_>>();
    entries(key, &values, &[])
}

fn project(name: &str, p: &ProjectConfig) -> ConfigSection {
    let checks = p.checks.as_ref();
    let review = match p.colleague_review {
        ColleagueReview::Standard => "standard",
        ColleagueReview::Full => "full",
    };
    let mut fields = vec![
        text("colleague_review", review, "standard"),
        text("slots", p.slots, config::default_slots()),
        text("checks.command", checks.map_or("", |c| &c.command), ""),
        text("checks.folder", checks.map_or("", |c| &c.folder), ""),
    ];
    for read in Read::ALL {
        let pass = p.passes.get(&read).cloned().unwrap_or_default();
        let key = |k| format!("passes.{}.{k}", read.spec().name);
        let mode = match pass.mode {
            Trigger::Manual => "manual",
            Trigger::Scheduled => "scheduled",
            Trigger::Auto => "auto",
        };
        fields.push(text(&key("mode"), mode, "manual"));
        for (name, v) in
            [("landings", pass.landings), ("edits", pass.edits), ("notes", pass.notes), ("days", pass.days)]
        {
            fields.push(text(&key(name), v.map(|n| n.to_string()).unwrap_or_default(), ""));
        }
        if read == Read::Dream {
            fields.push(text(&key("budget"), pass.budget.unwrap_or(DEFAULT_DREAM_BUDGET), DEFAULT_DREAM_BUDGET));
        }
    }
    fields.extend([entries("domains", &p.domains, &[]), ports("ports", &p.ports), ports("host_ports", &p.host_ports)]);
    ConfigSection { section: "project".into(), project: Some(name.into()), fields }
}

pub(super) fn build(c: &Config, secrets: &Secrets, stale: bool) -> ConfigView {
    let mut sections: Vec<_> = c
        .ordered_projects()
        .filter(|id| secrets.hit(id.as_str()).is_none())
        .filter_map(|id| c.projects.get(id).map(|p| project(id.as_str(), p)))
        .collect();
    sections.extend(super::seats::sections(c));
    sections.extend(system(c));
    for group in &mut sections {
        group.fields.retain(|f| secrets.hit(&f.key).is_none());
        for f in &mut group.fields {
            f.value = f.value.take().filter(|v| !secrets.contains_value(&serde_json::json!(v)));
            f.default = f.default.take().filter(|v| !secrets.contains_value(&serde_json::json!(v)));
            f.offers.retain(|v| !secrets.contains_value(&serde_json::json!(v)));
        }
    }
    ConfigView { sections, stale }
}
