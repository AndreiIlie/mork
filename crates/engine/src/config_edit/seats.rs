use mork_types::config::view::{ConfigField, ConfigSection, ConfigValue, SeatDefault};
use mork_types::config::{
    Cli, Config, Effort, MODEL_CATALOG, OPERATOR_MODELS, PickModel, SEAT_ROWS, Seat, Standing, Wall, bench,
};

fn offers(group: &str, wall: Wall) -> Vec<PickModel> {
    let extra = if group == "seats" || group == "pass" { OPERATOR_MODELS } else { &[] };
    MODEL_CATALOG
        .iter()
        .chain(extra)
        .filter(|(cli, _, _)| wall != Wall::None || *cli != Cli::Agy)
        .filter_map(|&(cli, model, label)| {
            let efforts: Vec<_> = cli
                .efforts(model)
                .iter()
                .copied()
                .filter(|e| match group {
                    "seats" => true,
                    "pass" => bench::benchmark(model, *e).is_some(),
                    _ => Effort::PICKED.contains(e),
                })
                .collect();
            (!efforts.is_empty()).then(|| PickModel { cli, model: model.into(), label: label.into(), efforts })
        })
        .collect()
}

fn selection(s: &Seat, configured: bool) -> ConfigValue {
    ConfigValue::Seat(SeatDefault { cli: s.cli, model: s.model.clone(), effort: s.effort, configured })
}

fn seat_field(c: &Config, fallback: &Config, name: &str, at: Standing) -> Option<ConfigField> {
    let current = c.seat_in(name, at)?;
    let configured = c.seats.get(name).is_some_and(|o| {
        o.cli.is_some()
            || o.model.is_some()
            || o.effort.is_some()
            || o.at(at).is_some_and(|s| s.cli.is_some() || s.model.is_some() || s.effort.is_some())
    });
    let mut choices = offers("seats", current.wall);
    if !choices.iter().any(|m| m.cli == current.cli && m.model == current.model) {
        choices.push(PickModel {
            cli: current.cli,
            model: current.model.clone(),
            label: current.label.clone(),
            efforts: current.cli.efforts(&current.model).to_vec(),
        });
    }
    Some(ConfigField {
        key: format!("{name}.{}", at.part()),
        value: Some(selection(&current, configured)),
        default: Some(selection(&fallback.seat_in(name, at)?, false)),
        offers: choices,
    })
}

pub(super) fn sections(c: &Config) -> [ConfigSection; 2] {
    let mut fallback = c.clone();
    for o in fallback.seats.values_mut() {
        o.cli = None;
        o.model = None;
        o.effort = None;
        o.work = None;
        o.report = None;
        o.inside = None;
        o.outside = None;
    }
    let seats = SEAT_ROWS.iter().filter_map(|&(name, at)| seat_field(c, &fallback, name, at)).collect();
    let picks = c.picks.clone().unwrap_or_default();
    let groups = [
        ("researcher", picks.researcher),
        ("reviewer.inside", picks.reviewer.inside),
        ("reviewer.outside", picks.reviewer.outside),
        ("pass", picks.pass),
    ];
    let picks = groups
        .into_iter()
        .map(|(key, models)| ConfigField {
            key: key.into(),
            value: Some(ConfigValue::Picks(models)),
            default: Some(ConfigValue::Picks(vec![])),
            offers: offers(key, Wall::Full),
        })
        .collect();
    [
        ConfigSection { section: "seats".into(), project: None, fields: seats },
        ConfigSection { section: "picks".into(), project: None, fields: picks },
    ]
}
