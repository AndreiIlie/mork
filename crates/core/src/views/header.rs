use mork_types::CardId;
use mork_types::about::CardAbout;
use mork_types::card::{Card, Mode};
use mork_types::view::{CardHeader, Fact, Times};

use super::findings::folded_parent;
use super::outside::Outside;
use super::pipeline::{build_pipeline, stage_times};
use super::timeline;
use crate::decide::Ctx;
use crate::messages::places;
use crate::state::State;

pub(super) fn header(state: &State, card: &Card, outside: &Outside, ctx: Ctx<'_>) -> CardHeader {
    let points = outside.timelines.get(&card.id).map(Vec::as_slice).unwrap_or_default();
    let t = timeline::totals(points, ctx.now);
    let times = Times { stages: stage_times(card.mode, &t), waiting_on_you: t.you, paused: t.paused };
    let ended = !card.stage.is_live();
    let spec = places::spec_room(&card.id);
    let spec = (places::two_rooms(card) && state.places.contains_key(spec.as_str())).then(|| spec.into());
    CardHeader {
        id: card.id.clone(),
        title: card.title.clone(),
        mode: card.mode,
        stage: card.stage,
        paused: card.paused,
        pipeline: build_pipeline(state, card, points, outside, ctx.config),
        times,
        facts: facts(card, outside, ctx),
        cost_cents: outside.card_costs.get(&card.id).copied().filter(|_| ended),
        clone: outside.clones.get(&card.id).cloned(),
        about: about(state, card, outside, ctx),
        spec,
        parent: folded_parent(state, card),
    }
}

fn about(state: &State, card: &Card, outside: &Outside, ctx: Ctx<'_>) -> CardAbout {
    let project = ctx.config.projects.get(&card.project);
    CardAbout {
        project: card.project.clone(),
        branch: card.branch.clone(),
        base: project.map(|p| p.base.clone()).unwrap_or_default(),
        fork: card.base_tip.as_ref().or(card.fork.as_ref()).map(|s| s.short().to_owned()),
        pr: card.pr,
        pr_url: card.pr.and_then(|n| pr_url(state, card, n, ctx)),
        run_cost_cents: outside.card_costs.get(&card.id).copied(),
        run_cost_unknown: outside.card_costs_unknown.contains(&card.id),
        seat_costs: outside.seat_costs.get(&card.id).cloned().unwrap_or_default(),
        delegated: card.delegated,
        self_sustained: card.delegated && card.self_sustained,
        switches: super::offers::switches(state, card, ctx),
    }
}

// Its page as the poller read it, else as Bitbucket makes them.
fn pr_url(state: &State, card: &Card, n: u64, ctx: Ctx<'_>) -> Option<String> {
    let read = state.world.open_prs.get(&card.project).and_then(|prs| prs.iter().find(|p| p.pr == n));
    if let Some(p) = read.filter(|p| !p.link.is_empty()) {
        return Some(p.link.clone());
    }
    ctx.config.projects.get(&card.project)?.forge.pr_link(n)
}

fn facts(card: &Card, outside: &Outside, ctx: Ctx<'_>) -> Vec<Fact> {
    let base = ctx.config.projects.get(&card.project).map(|p| p.base.clone()).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(h) = &card.head {
        out.push(Fact::Head { head: h.short().to_owned() });
    }
    if let Some(j) = &card.join {
        out.push(Fact::JoinOwed { base: j.base.clone(), tip: j.tip.short().to_owned(), asked: j.asked_of.is_some() });
    }
    if let Some(n) = outside.base_ahead.get(&card.id).filter(|n| **n > 0) {
        out.push(Fact::BaseAhead { base, commits: *n });
    }
    for (other, files) in &card.overlaps {
        out.push(Fact::Overlap { card: CardId::new(other.as_str()), files: files.len() });
    }
    if card.mode != Mode::Full {
        out.push(Fact::Mode { mode: card.mode });
    }
    if card.run > 1 {
        out.push(Fact::Run { run: card.run });
    }
    if !card.carries.is_empty() {
        out.push(Fact::Carries { cards: card.carries.iter().map(|k| k.id.clone()).collect() });
    }
    out
}
