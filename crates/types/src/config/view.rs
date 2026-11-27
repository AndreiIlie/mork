use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Cli, Effort, PickModel};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatDefault {
    pub cli: Cli,
    pub model: String,
    pub effort: Effort,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum ConfigValue {
    Text(String),
    Entries(Vec<String>),
    Seat(SeatDefault),
    Picks(Vec<PickModel>),
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigField {
    pub key: String,
    pub value: Option<ConfigValue>,
    pub default: Option<ConfigValue>,
    pub offers: Vec<PickModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSection {
    pub section: String,
    pub project: Option<String>,
    pub fields: Vec<ConfigField>,
}

#[derive(Debug, Serialize)]
pub struct ConfigView {
    pub sections: Vec<ConfigSection>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldChange {
    pub read: ConfigValue,
    pub value: ConfigValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigChange {
    pub section: String,
    pub project: Option<String>,
    pub fields: BTreeMap<String, FieldChange>,
}
