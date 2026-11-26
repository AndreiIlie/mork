//! A card's visual mode (fac-258): the operator's switch on a live full card, any time; an orchestrator's only
//! as it starts one (`start --visual`).

use mork_types::CardId;
use mork_types::card::{Card, MockStep, Mode, Stage};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// `visual <card>`, or with `off` its end: at brief it opens the baseline step or ends the mock; at build only an
/// open mock ends and starts the build.
pub(super) fn toggle(state: &State, card: &CardId, on: bool, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(c) = state.cards.get(&id) else {
        return Decision::refused(format!("no card `{card}` in the state: a started card takes visual mode"));
    };
    if let Err(why) = takes(c) {
        return Decision::refused(why);
    }
    let mut p = Poster::for_ctx(state, ctx);
    p.journal("visual", Some(id.as_str()), json!({"visual": on}));
    if c.visual != on {
        let (level, rule) = toggle_rule(c, on);
        let text = format!("The operator turned {id}'s visual mode {level}.\n{rule}");
        crate::messages::requests::tell_agent(&mut p, &id, super::facts::holder(c), "Visual mode changed", &text);
    }
    let was = c.visual;
    let c = Card { visual: on, ..c.clone() };
    if !on && super::mock::open(&c) {
        super::mock::end(&mut p, c, MockStep::Skipped);
    } else if !on && was && at_brief(&c) {
        super::baseline::turned_off(&mut p, c, ctx);
    } else if on && !was && at_brief(&c) && c.mock.is_none_or(|m| m.step == MockStep::Skipped) {
        super::baseline::turned_on(&mut p, c, ctx);
    } else if on && !was && at_brief(&c) && c.recap && p.all_asks().any(|a| super::brief::is_given_plan(a, &id)) {
        // A visual card's plan is the operator's alone: replace a given plan ask with a fresh ungiven one that notifies him.
        let mut c = Card { recap: false, ..c };
        super::brief::offer(&mut p, &mut c, ctx, None);
        p.put_card(c);
    } else {
        p.put_card(c);
    }
    let mut d = Decision::ok(json!({"visual": {"card": id.as_str(), "visual": on}}));
    p.finish(&mut d);
    d
}

fn toggle_rule(c: &Card, on: bool) -> (&'static str, String) {
    if !on {
        return (
            "off",
            "The mock ends: a baseline under way stops, and the plan's approve, or an open mock's at build, starts the \
             build without it."
                .to_owned(),
        );
    }
    let (seat, next) = if c.no_planner { ("your", "you agree") } else { ("the foreman's", "the planner agrees") };
    (
        "on",
        format!(
            "Before the plan's approve, {seat} baseline seat reproduces today's pages in the mock's folder, with any waiting \
             recap withdrawn. Then {next} on the visual direction before editing the mock. The operator approves the \
             plan and mock together. Nothing moves on a plan already approved."
        ),
    )
}

// Visual mode's brief-time mock opens or ends only while the specification is agreed (fac-636).
fn at_brief(c: &Card) -> bool {
    c.stage == Stage::Brief
}

// Only a live full card has a visual mode: report, spec and review cards build nothing to look at (fac-688).
fn takes(c: &Card) -> Result<(), String> {
    if c.mode != Mode::Full {
        return Err(format!(
            "{} is a {} card: only a full card has a visual mode",
            c.id,
            crate::lifecycle::facts::mode_word(c.mode)
        ));
    }
    if !c.stage.is_live() {
        return Err(format!("{} has ended: it is {}", c.id, c.stage.as_str()));
    }
    Ok(())
}

/// A run's visual mode: asked by its start, or carried from the card's last run, on a full card alone.
pub(super) fn of_run(run: &Card, asked: bool, prior: Option<&Card>) -> Result<bool, String> {
    if asked && run.mode != Mode::Full {
        return Err(format!(
            "`--visual` is a full card's: a {} card has no visual mode",
            crate::lifecycle::facts::mode_word(run.mode)
        ));
    }
    Ok(run.mode == Mode::Full && (asked || prior.is_some_and(|c| c.visual)))
}
