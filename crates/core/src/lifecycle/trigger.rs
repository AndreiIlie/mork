use std::cell::OnceCell;

use mork_types::ProjectId;
use mork_types::config::{SeatPhase, Standing};
use mork_types::lookback::{Baseline, Counts, PassConfig, Read, Trigger};
use mork_types::note::NoteWhat;
use serde::Serialize;

use super::lookback::{self, Start};
use crate::agents::turns;
use crate::decide::Ctx;
use crate::post::Poster;
use crate::routing::choose::{Candidate as Pair, Inputs, Request};
use crate::state::{Change, State};

#[derive(Clone)]
pub(crate) struct Offer {
    pub pair: Pair,
    pub row: serde_json::Value,
}

pub(crate) fn offer(
    state: &State,
    project: &ProjectId,
    key: &str,
    name: &str,
    at: Standing,
    ctx: Ctx<'_>,
) -> Option<Offer> {
    let seat = ctx.config.seat_in(name, at)?;
    let req = Request {
        agent: key,
        run: 1,
        phase: at.phase,
        role: crate::routing::seats::role(name, at.phase),
        leaf: Some(project.as_str()),
        partner: None,
    };
    let rec = Inputs::new(&state.limits, ctx).choose(&req, &seat, &ctx.config.picks.as_ref()?.pass, name == "foreman");
    Some(Offer { pair: rec.best()?.clone(), row: rec.row(&req, mork_types::config::RoutingMode::Auto) })
}

pub(crate) fn starting_offer(state: &State, project: &ProjectId, ctx: Ctx<'_>) -> Option<Offer> {
    let at = Standing { phase: SeatPhase::Report, review: None };
    let key = format!("{}/foreman", lookback::next_id(state, project.as_str()));
    offer(state, project, &key, "foreman", at, ctx)
}

pub(crate) fn bar(pair: &Offer) -> u32 {
    100 - pair.pair.offer.spare
}

fn idle(p: &Poster<'_>) -> bool {
    !p.state.cards.values().any(|c| c.stage.is_live() && lookback::automatic(c))
        && !turns::real_running(p.state)
        && !p.all_agents().any(|a| turns::real_owed(p.state, a))
}

struct Candidate<'a> {
    project: &'a ProjectId,
    read: Read,
    score: Score,
    margin: f64,
    pair: Offer,
}

pub(crate) fn due(p: &mut Poster<'_>, ctx: Ctx<'_>) {
    let mut winner = None;
    for project in ctx.config.ordered_projects() {
        let passes = &ctx.config.projects[project].passes;
        if passes.is_empty() || lookback::live(p.state, project).is_some() {
            continue;
        }
        let offer = OnceCell::new();
        let mut scheduled = None;
        let mut candidate = None;
        for (&read, pass) in passes.iter().filter(|(_, pass)| pass.mode != Trigger::Manual) {
            let base = baseline(p.state, project, read, ctx.now);
            if !p.state.baselines.contains_key(&(project.clone(), read)) {
                p.other.push(Change::PutBaseline(base.clone()));
            }
            if pass.mode == Trigger::Scheduled {
                if let Some(met) = threshold(p, &base, pass, ctx) {
                    scheduled.get_or_insert((read, met));
                }
            } else if let Some(pair) =
                offer.get_or_init(|| idle(p).then(|| starting_offer(p.state, project, ctx)).flatten())
            {
                let score = score(p.state, &base, ctx);
                let margin = score.total - f64::from(bar(pair));
                if score.active() && margin > 0.0 {
                    prefer(&mut candidate, Candidate { project, read, score, margin, pair: pair.clone() });
                }
            }
        }
        if let Some((read, met)) = scheduled {
            lookback::begin(p, project, read, Start::Scheduled(met), ctx);
        } else if let Some(c) = candidate {
            prefer(&mut winner, c);
        }
    }
    if let Some(c) = winner {
        lookback::begin(p, c.project, c.read, Start::Auto(&c.pair, &c.score), ctx);
    }
}

fn prefer<'a>(best: &mut Option<Candidate<'a>>, next: Candidate<'a>) {
    if best.as_ref().is_none_or(|b| next.margin > b.margin) {
        *best = Some(next);
    }
}

pub(crate) fn baseline(state: &State, project: &ProjectId, read: Read, now: i64) -> Baseline {
    if let Some(b) = state.baselines.get(&(project.clone(), read)) {
        return b.clone();
    }
    let cursor = state.lookbacks.get(project).and_then(|reads| reads.get(&read));
    Baseline {
        project: project.clone(),
        read,
        at_ms: cursor.map_or(now, |c| c.ended_ms),
        counts: lookback::counts(state, project),
    }
}

fn notes<'a>(state: &'a State, base: &'a Baseline, ctx: Ctx<'a>) -> impl Iterator<Item = ()> + 'a {
    state
        .notes
        .range((std::ops::Bound::Excluded(base.counts.note), std::ops::Bound::Unbounded))
        .filter(|(_, n)| {
            matches!(n.what, NoteWhat::Lesson { .. })
                && crate::memory::under(n, ctx.config.mork.project.as_ref()) == Some(&base.project)
        })
        .map(|_| ())
}

fn threshold(p: &Poster<'_>, base: &Baseline, pass: &PassConfig, ctx: Ctx<'_>) -> Option<&'static str> {
    let Counts { landings, memory_edits, .. } = lookback::counts(p.state, &base.project);
    let days = p.now.saturating_sub(base.at_ms).max(0).unsigned_abs() / 86_400_000;
    for (name, count, limit) in [
        ("landings", landings.saturating_sub(base.counts.landings), pass.landings),
        ("edits", memory_edits.saturating_sub(base.counts.memory_edits), pass.edits),
        ("days", days, pass.days),
    ] {
        if limit.is_some_and(|n| count >= n) {
            return Some(name);
        }
    }
    let needed = usize::try_from(pass.notes?).ok()?;
    (notes(p.state, base, ctx).take(needed).count() == needed).then_some("notes")
}

#[derive(Serialize)]
pub(crate) struct Score {
    #[serde(skip_serializing_if = "Option::is_none")]
    landings: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edits: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<f64>,
    days: f64,
    total: f64,
}

impl Score {
    fn active(&self) -> bool {
        [self.landings, self.edits, self.notes].into_iter().flatten().any(|part| part > 0.0)
    }
}

pub(crate) fn score(state: &State, base: &Baseline, ctx: Ctx<'_>) -> Score {
    let counts = lookback::counts(state, &base.project);
    let elapsed = ctx.now.saturating_sub(base.at_ms).max(0) as f64 / 86_400_000.0;
    let part = |x, half| 100.0 * x / (x + half);
    let landed = counts.landings.saturating_sub(base.counts.landings) as f64;
    let (landings, edits, notes, days) = match base.read {
        Read::Pulse => (Some(part(landed, 20.0)), None, None, part(elapsed, 7.0)),
        Read::DeepSweep => (Some(part(landed, 200.0)), None, None, part(elapsed, 30.0)),
        Read::Dream => {
            let edits = counts.memory_edits.saturating_sub(base.counts.memory_edits) as f64;
            let new_notes = notes(state, base, ctx).count() as f64;
            (None, Some(part(edits, 20.0)), Some(part(new_notes, 5.0)), part(elapsed, 14.0))
        }
    };
    let total = days + landings.unwrap_or_default() + edits.unwrap_or_default() + notes.unwrap_or_default();
    Score { landings, edits, notes, days, total }
}
