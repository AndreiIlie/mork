use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use mork_types::Sha;
use mork_types::card::Card;
use mork_types::lookback::{INPUT_FILE, Read, Run};
use serde_json::{Value, json};

use super::Engine;
use crate::git::{self, mirror};

impl Engine {
    pub(super) fn write_lookback(&self, card: &Card, run: &Run, at: &str, dir: &Path) -> Result<Option<Sha>, String> {
        let mut inputs = json!({"read": run.read, "through": at, "started_ms": card.started_ms});
        if run.read == Read::Dream {
            let pass = self.ctx.config.projects.get(&card.project).and_then(|p| p.passes.get(&Read::Dream));
            inputs["budget"] = json!(pass.and_then(|p| p.budget).unwrap_or(60));
            inputs["last"] = json!(self.state.lookbacks.get(&card.project).and_then(|m| m.get(&Read::Dream)));
        } else {
            inputs["open_cards"] = json!(self.open_cards(card));
        }
        let from = if run.read == Read::Pulse {
            let since = run.ended_since_ms.ok_or("pulse has no starting time for ended cards")?;
            let ended = self.store.lookback_ends(&card.project, since, card.started_ms).map_err(|e| e.to_string())?;
            inputs["ended_since_ms"] = json!(since);
            inputs["ended_cards"] = json!(ended);
            inputs["tool_failures"] =
                json!(self.store.tool_failures(&card.project, since, card.started_ms).map_err(|e| e.to_string())?);
            self.merged_inputs(card, run, at, &mut inputs)?
        } else {
            None
        };
        let path = Path::new(INPUT_FILE);
        if let Some(parent) = path.parent() {
            mork_sys::mkdir_beneath(dir, parent).map_err(|e| format!("{INPUT_FILE}: {e}"))?;
        }
        let bytes = serde_json::to_vec_pretty(&inputs).map_err(|e| e.to_string())?;
        mork_sys::write_beneath(dir, path, &bytes).map_err(|e| format!("{INPUT_FILE}: {e}"))?;
        Ok(from)
    }

    fn merged_inputs(&self, card: &Card, run: &Run, at: &str, inputs: &mut Value) -> Result<Option<Sha>, String> {
        let landings = self.store.lookback_landings(&card.project).map_err(|e| e.to_string())?;
        let (mut placed, unplaced): (Vec<_>, Vec<_>) = landings.into_iter().partition(|l| l.after.is_some());
        let mut from = run.from.clone();
        if !placed.is_empty() {
            let home = &self.opts.home;
            let repo = mirror::ensure(home, &card.project)?;
            from = from.or_else(|| {
                let cutoff = run.since_ms?;
                placed
                    .iter()
                    .rev()
                    .filter(|l| l.time_ms < cutoff)
                    .filter_map(|l| l.after.as_deref())
                    .find(|sha| git::descends(home, &repo, sha, at) == Some(true))
                    .map(Sha::new)
            });
            let range = from.as_ref().map_or_else(|| at.to_owned(), |from| format!("{from}..{at}"));
            let revs = git::git(home, &repo, &["rev-list", "--end-of-options", &range, "--"], Duration::from_secs(20))?;
            let commits: BTreeSet<&str> = revs.lines().collect();
            placed.retain(|l| l.after.as_deref().is_some_and(|sha| commits.contains(sha)));
        }
        inputs["from"] = json!(from);
        inputs["since_ms"] = json!(run.since_ms);
        inputs["landings"] = json!(placed);
        inputs["unplaced_landings"] = json!(unplaced);
        Ok(from)
    }

    fn open_cards(&self, card: &Card) -> Vec<Value> {
        self.state
            .board
            .values()
            .filter(|b| b.project == card.project && b.file.starts_with("tasks/") && b.status != b.done_status)
            .map(|b| {
                json!({
                    "card": b.id, "title": b.title, "status": b.status,
                    "description": b.description, "acceptance": b.acceptance,
                })
            })
            .collect()
    }
}
