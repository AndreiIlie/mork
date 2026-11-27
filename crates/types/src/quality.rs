use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct QualityGroup {
    pub project: String,
    pub role: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Default, Serialize)]
pub struct FirstPass {
    pub verified: usize,
    pub first_pass: usize,
    pub first_pass_percent: Option<f64>,
    pub landed: usize,
    pub median_rounds: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct Precision {
    pub findings: usize,
    pub fixed: usize,
    pub declined: usize,
    pub covered: usize,
    pub stalled: usize,
    pub fixed_percent: Option<f64>,
    pub declined_percent: Option<f64>,
    pub stalemate_percent: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct QualitySpend {
    pub cards: usize,
    pub average_dollars: f64,
    pub unknown: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct FailureRate {
    pub turns: usize,
    pub calls: u64,
    pub failed: u64,
    pub percent: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct BuilderRow {
    pub group: QualityGroup,
    pub counts: FirstPass,
}

#[derive(Debug, Serialize)]
pub struct FindingRow {
    pub group: QualityGroup,
    pub lens: String,
    pub counts: Precision,
}

#[derive(Debug, Serialize)]
pub struct SpendRow {
    pub group: QualityGroup,
    pub spend: QualitySpend,
}

#[derive(Debug, Serialize)]
pub struct ToolRow {
    pub group: QualityGroup,
    pub counts: FailureRate,
}

#[derive(Debug, Default, Serialize)]
pub struct Quality {
    pub since_ms: i64,
    pub builders: Vec<BuilderRow>,
    pub findings: Vec<FindingRow>,
    pub spend: Vec<SpendRow>,
    pub tools: Vec<ToolRow>,
}

#[derive(Debug, Serialize)]
pub struct QualityAnswer {
    pub quality: Quality,
}
