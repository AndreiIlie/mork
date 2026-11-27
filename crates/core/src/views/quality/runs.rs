use super::{Event, Histories, History, cost_group, group, percent};
use crate::views::burn::{parent_of, run_start};
use mork_types::quality::{BuilderRow, FirstPass, QualityGroup, QualitySpend, SpendRow};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Run<'a> {
    pub(crate) rows: &'a [Event],
    pub(crate) mode: &'a str,
    pub(crate) parent: Option<(&'a str, i64)>,
    pub(crate) start: i64,
}

pub(crate) fn runs(history: &History) -> Vec<Run<'_>> {
    let mut starts = Vec::new();
    let mut current = 1;
    for (i, e) in history.rows.iter().enumerate() {
        if let Some(n) = run_start(&e.kind, &e.data, current) {
            if starts.is_empty() || n != current {
                starts.push(i);
            }
            current = n;
        }
    }
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    starts.push(history.rows.len());
    let mut mode = history.mode.as_str();
    starts
        .windows(2)
        .filter_map(|pair| {
            let rows = &history.rows[pair[0]..pair[1]];
            let start = rows.first()?.seq;
            if let Some(m) = rows.iter().find(|e| e.kind == "card.start").and_then(|e| e.text("mode")) {
                mode = m
            }
            let parent = rows.iter().find_map(|e| parent_of(&e.kind, &e.data).map(|id| (id, e.seq)));
            Some(Run { rows, mode, parent, start })
        })
        .collect()
}

pub(crate) fn builder<'a>(rows: &'a [Event], asked: Option<&Event>) -> Option<&'a Event> {
    let asked = asked?;
    let mut pending = None;
    let mut submissions = BTreeMap::new();
    let mut built = Vec::new();
    let mut cost = None;
    for e in rows.iter().take_while(|e| e.seq < asked.seq) {
        match e.kind.as_str() {
            "cost" if e.text("agent").is_some_and(|a| a.ends_with("/foreman")) => cost = Some(e),
            "reply" if e.text("outcome") == Some("built") && e.text("agent") == cost.and_then(|r| r.text("agent")) => {
                built.push(cost)
            }
            "build.requested" => pending = cost.filter(|r| r.text("job") == e.text("job")),
            "build.submitted" => {
                if let Some(head) = e.text("head") {
                    submissions.insert(head, pending);
                }
            }
            _ => {}
        }
    }
    if let Some(head) = asked.text("submitted") {
        return submissions.get(head).copied().flatten();
    }
    if let [only] = built.as_slice() { *only } else { None }
}

fn rounds_before(reviews: &[&Event], seq: i64) -> usize {
    reviews.iter().filter(|e| e.kind == "review.verdict" && e.seq < seq).count()
}

pub(crate) fn verified<'a>(run: &Run<'a>, reviews: &[&Event]) -> Option<(&'a Event, bool)> {
    let event = run.rows.iter().find(|e| e.kind == "merge.tested")?;
    Some((event, rounds_before(reviews, event.seq) == 1))
}

#[derive(Default)]
struct Totals {
    builders: BTreeMap<QualityGroup, (FirstPass, Vec<usize>)>,
    spend: BTreeMap<QualityGroup, (u128, usize, bool)>,
}

impl Totals {
    fn count(&mut self, since: i64, card: &History, run: &Run<'_>, reviews: &[&Event]) {
        let verified = verified(run, reviews).filter(|(e, _)| e.time >= since);
        let landed = run.rows.iter().find(|e| e.kind == "card.landed").filter(|e| e.time >= since);
        if verified.is_none() && landed.is_none() {
            return;
        }
        let first = reviews.iter().copied().find(|e| e.kind == "review.asked");
        let key = group(&card.project, "foreman", builder(run.rows, first));
        if let Some((_, first_pass)) = verified {
            let (row, _) = self.builders.entry(key.clone()).or_default();
            row.verified += 1;
            row.first_pass += usize::from(first_pass);
        }
        if let Some(e) = landed {
            let n = rounds_before(reviews, e.seq);
            if n > 0 {
                let (row, samples) = self.builders.entry(key).or_default();
                row.landed += 1;
                samples.push(n);
            }
            self.spend(card, run, reviews);
        }
    }

    fn spend(&mut self, card: &History, run: &Run<'_>, reviews: &[&Event]) {
        let costs = run.rows.iter().map(|e| (e, false)).chain(reviews.iter().map(|&e| (e, true)));
        let mut worked = BTreeSet::new();
        for (e, review) in costs.filter(|(e, _)| e.kind == "cost") {
            let key = cost_group(card, e, review);
            let first = worked.insert(key.clone());
            let (total, cards, unknown) = self.spend.entry(key).or_default();
            *total += u128::from(e.data["delta"].as_u64().unwrap_or_default());
            *cards += usize::from(first);
            *unknown |= e.data["unknown"].as_bool().unwrap_or(false);
        }
    }

    fn finish(self) -> (Vec<BuilderRow>, Vec<SpendRow>) {
        let builders = self
            .builders
            .into_iter()
            .map(|(group, (mut counts, mut rounds))| {
                rounds.sort_unstable();
                let n = rounds.len();
                counts.median_rounds = (n > 0).then(|| (rounds[(n - 1) / 2] as f64 + rounds[n / 2] as f64) / 2.0);
                counts.first_pass_percent = percent(counts.first_pass, counts.verified);
                BuilderRow { group, counts }
            })
            .collect();
        let spend = self
            .spend
            .into_iter()
            .map(|(group, (total, cards, unknown))| SpendRow {
                group,
                spend: QualitySpend { cards, unknown, average_dollars: total as f64 / cards as f64 / 1_000_000.0 },
            })
            .collect();
        (builders, spend)
    }
}

pub(crate) fn with_runs(cards: &Histories, mut visit: impl FnMut(&str, &Run<'_>, &[&Event])) {
    let runs: BTreeMap<_, _> = cards.iter().map(|(id, history)| (id.as_str(), runs(history))).collect();
    let mut linked = BTreeMap::<_, Vec<&Event>>::new();
    for review in runs.values().flatten() {
        let Some((parent, at)) = review.parent else { continue };
        let Some(owner) = runs.get(parent).and_then(|all| all.iter().rev().find(|r| r.start <= at)) else { continue };
        // A review's closing turns can settle after its parent starts another run.
        linked.entry((parent, owner.start)).or_default().extend(review.rows.iter().filter(|e| e.seq >= owner.start));
    }
    for (id, runs) in runs {
        for run in runs.iter().filter(|r| r.mode == "full") {
            let mut children = linked.remove(&(id, run.start)).unwrap_or_default();
            children.sort_by_key(|e| e.seq);
            visit(id, run, &children);
        }
    }
}

pub(super) fn aggregate(since: i64, cards: &Histories) -> (Vec<BuilderRow>, Vec<SpendRow>) {
    let mut totals = Totals::default();
    with_runs(cards, |id, run, children| totals.count(since, &cards[id], run, children));
    totals.finish()
}
