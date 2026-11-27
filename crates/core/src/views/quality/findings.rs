use super::{Event, Histories, History, group, percent};
use mork_types::quality::{FindingRow, Precision, QualityGroup};
use mork_types::review::{Raised, Review};
use std::collections::{BTreeMap, BTreeSet};

struct Finding {
    group: QualityGroup,
    lens: String,
    fixed: bool,
    declined: bool,
    covered: bool,
    stalled: bool,
}

type Groups = BTreeMap<(QualityGroup, String), Precision>;

fn generations(rows: &[Event]) -> Vec<&[Event]> {
    let mut linked = false;
    let mut start = 0;
    let mut generations = Vec::new();
    for (i, e) in rows.iter().enumerate() {
        if e.kind == "card.start" {
            let reset = e.text("of").is_some() || (linked && e.text("branch").is_some());
            if reset && i > start {
                generations.push(&rows[start..i]);
                start = i;
            }
            linked = e.text("of").is_some();
        } else if e.kind == "review.adopted" {
            linked = true;
        }
    }
    generations.push(&rows[start..]);
    generations
}

fn stored<'a>(snapshot: Option<&'a Review>, event: &Event) -> impl Iterator<Item = &'a Raised> {
    let matching = snapshot.filter(|review| {
        review
            .rounds
            .iter()
            .any(|r| Some(u64::from(r.n)) == event.round() && Some(r.commit.as_str()) == event.text("commit"))
    });
    matching.into_iter().flat_map(|r| &r.raised).filter(move |f| Some(u64::from(f.round)) == event.round())
}

#[derive(Default)]
struct Origin<'a> {
    topics: BTreeMap<&'a str, String>,
    costs: BTreeMap<&'a str, &'a Event>,
    reports: BTreeMap<&'a str, &'a Event>,
}

impl<'a> Origin<'a> {
    fn observe(&mut self, row: &'a Event) {
        let agent = row.text("agent").and_then(|s| s.rsplit('/').next()).unwrap_or("");
        match row.kind.as_str() {
            "review.asked" => self.reports.clear(),
            "cost" => {
                self.costs.insert(agent, row);
            }
            "child.spawned" => {
                if let Some(t) = row.text("topic") {
                    self.topics.insert(agent, t.to_lowercase());
                }
            }
            "reply" if row.text("outcome") == Some("review-fail") => {
                if let Some(&cost) = self.costs.get(agent) {
                    self.reports.insert(agent, cost);
                }
            }
            _ => {}
        }
    }

    fn finding(&self, project: &str, by: &str, covered: bool) -> Finding {
        let role = if by == "foreman" { "review foreman" } else { "reviewer" };
        let lens = match (covered, by) {
            (false, _) => "unknown",
            (true, "foreman") => "foreman",
            _ => self.topics.get(by).map_or("unknown", String::as_str),
        };
        Finding {
            group: group(project, role, self.reports.get(by).copied()),
            lens: lens.into(),
            fixed: false,
            declined: false,
            covered,
            stalled: false,
        }
    }

    fn record(&self, findings: &mut BTreeMap<u64, Finding>, project: &str, e: &Event, snapshot: Option<&Review>) {
        if let Some(named) = e.data["named"].as_array() {
            for row in named.iter().filter(|f| f["round"].as_u64() == e.round()) {
                if let (Some(n), Some(by)) = (row["n"].as_u64(), row["by"].as_str()) {
                    findings.entry(n).or_insert_with(|| self.finding(project, by, true));
                }
            }
        } else {
            for f in stored(snapshot, e) {
                let mut row = self.finding(project, &f.by, false);
                row.declined = f.declined;
                row.fixed = f.resolved && !f.declined && !f.withdrawn && f.dropped.is_none();
                findings.insert(u64::from(f.n), row);
            }
        }
    }
}

fn apply_outcome(found: &mut BTreeMap<u64, Finding>, e: &Event) {
    if e.kind == "review.resolved" {
        if let Some(f) = e.round().and_then(|n| found.get_mut(&n)).filter(|f| f.covered) {
            f.declined = e.data["declined"] == true;
            f.fixed = !f.declined && e.data["dropped"] != true;
        }
    } else if e.kind == "review.findings.withdrawn" {
        for n in numbers(e) {
            if let Some(f) = found.get_mut(&n) {
                f.fixed = false;
                f.declined = false;
            }
        }
    }
}

fn numbers(e: &Event) -> impl Iterator<Item = u64> + '_ {
    e.data["findings"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_u64)
}

fn collect(
    groups: &mut Groups,
    card: &History,
    since: i64,
    rows: &[Event],
    snapshot: Option<&Review>,
    stalled: &[&Event],
) {
    let mut found = BTreeMap::new();
    let mut origin = Origin::default();
    for e in rows {
        origin.observe(e);
        if e.kind == "review.verdict" && e.time >= since {
            origin.record(&mut found, &card.project, e, snapshot);
        }
        apply_outcome(&mut found, e);
    }
    let stalled: BTreeSet<_> = stalled.iter().flat_map(|e| numbers(e)).collect();
    for (n, mut f) in found {
        f.stalled = f.covered && stalled.contains(&n);
        let row = groups.entry((f.group, f.lens)).or_default();
        row.findings += 1;
        row.covered += usize::from(f.covered);
        row.stalled += usize::from(f.stalled);
        row.fixed += usize::from(f.fixed);
        row.declined += usize::from(f.declined);
    }
}

pub(super) fn aggregate(since: i64, cards: &Histories) -> Vec<FindingRow> {
    let mut stalls = BTreeMap::<_, Vec<&Event>>::new();
    for e in cards.values().flat_map(|c| &c.rows).filter(|e| e.kind == "review.stalemate") {
        if let Some(id) = e.text("review") {
            stalls.entry(id).or_default().push(e);
        }
    }
    let mut groups = Groups::new();
    for (id, card) in cards {
        if !card.rows.iter().any(|e| e.kind == "review.verdict" && e.time >= since) {
            continue;
        }
        let epochs = generations(&card.rows);
        let marks = stalls.entry(id.as_str()).or_default();
        marks.sort_by_key(|e| e.seq);
        for (i, rows) in epochs.iter().enumerate() {
            let start = rows.first().map_or(0, |e| e.seq);
            let end = epochs.get(i + 1).and_then(|r| r.first()).map_or(i64::MAX, |e| e.seq);
            let snapshot = card.legacy.as_ref().filter(|_| i + 1 == epochs.len());
            collect(
                &mut groups,
                card,
                since,
                rows,
                snapshot,
                &marks[marks.partition_point(|e| e.seq < start)..marks.partition_point(|e| e.seq < end)],
            );
        }
    }
    groups
        .into_iter()
        .map(|((group, lens), mut counts)| {
            let decided = counts.fixed + counts.declined;
            counts.fixed_percent = percent(counts.fixed, decided);
            counts.declined_percent = percent(counts.declined, decided);
            counts.stalemate_percent = percent(counts.stalled, counts.covered);
            FindingRow { group, lens, counts }
        })
        .collect()
}
