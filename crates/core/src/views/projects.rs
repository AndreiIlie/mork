// Projects stay visible before their first look-back and while a report is paused or still starting.

use factory_types::lookback::{LastView, ProjectView, Read, ReadView};

use crate::decide::Ctx;
use crate::state::State;

use super::outside::Outside;

pub(super) fn projects(state: &State, outside: &Outside, ctx: Ctx<'_>) -> Vec<ProjectView> {
    ctx.config
        .ordered_projects()
        .map(|project| {
            let live = crate::lifecycle::lookback::live(state, project);
            let completed = state.lookbacks.get(project);
            let rows = Read::ALL
                .into_iter()
                .map(|read| {
                    let last = completed.and_then(|reads| reads.get(&read)).map(|c| {
                        let commits = outside
                            .lookback_counts
                            .get(&(project.clone(), read))
                            .filter(|(commit, _)| c.commit.as_ref() == Some(commit))
                            .map(|(_, count)| *count);
                        last_view(c.ended_ms, outside.tz_offset_min, commits)
                    });
                    let running = live.filter(|(_, active)| *active == read).map(|(id, _)| id.clone());
                    ReadView { read, last, running }
                })
                .collect();
            ProjectView { project: project.clone(), rows }
        })
        .collect()
}

fn last_view(ended_ms: i64, offset_min: i32, commits: Option<u32>) -> LastView {
    let local = ended_ms.saturating_add(i64::from(offset_min) * 60_000);
    let (y, mo, d, h, mi, _) = crate::state::names::civil_from_ms(local);
    LastView { date: format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}"), ended_ms, commits }
}
