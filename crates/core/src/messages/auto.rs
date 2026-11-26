//! A project on auto (fac-381): the operator's switch saves a default, each card that starts in the project starts delegated.
//! It changes no card that exists, and asks follow each card's own toggle (fac-172).

use mork_types::orchestrator::Auto;
use mork_types::view::Offer;
use mork_types::{Command, ProjectId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::state::{Change, State};

/// `auto <project> [--off]`: the project's default for the cards that start from now on.
pub(super) fn toggle(project: &ProjectId, off: bool, ctx: Ctx<'_>) -> Decision {
    if !ctx.config.projects.contains_key(project) {
        return Decision::refused(format!("no project `{project}`"));
    }
    let mut d = Decision::ok(json!({"auto": {"project": project, "on": !off}}));
    d.journal("auto", None, json!({"project": project, "on": !off}));
    d.changes.push(Change::PutAuto(Auto { project: project.clone(), on: !off }));
    d
}

pub fn on(state: &State, project: &ProjectId) -> bool {
    state.autos.get(project).is_some_and(|a| a.on)
}

/// Main's toggle of each project's auto, worded as what a click does.
pub fn offers(state: &State, ctx: Ctx<'_>) -> Vec<Offer> {
    let config = ctx.config;
    let mut out = Vec::new();
    for project in config.ordered_projects() {
        let (label, off) = if on(state, project) {
            (format!("end auto {project}"), Some(true))
        } else {
            (format!("auto {project}"), None)
        };
        let command = Command::Auto { project: project.clone(), off };
        if crate::views::offers::accepted(state, &command, ctx) {
            out.push(Offer { label, command });
        }
    }
    out
}
