// Connection-bound host jobs have no gate result and no client left to recover after a restart (fac-480).

use factory_core::decide::Event;
use factory_types::job::JobKind;
use factory_types::state::{Job, Settlement};
use factory_types::work::Purpose;
use serde_json::{Value, json};

use super::Engine;
use super::work::{Plan, plan};

impl Engine {
    pub(super) fn plan_wall_run(&self, purpose: &Purpose) -> Result<Plan, String> {
        let Purpose::WallTests { card, command, .. } = purpose else { return Err("not a wall-test run".into()) };
        let c = self.card(card)?;
        let dir = self.clone_dir(card.as_str()).display().to_string();
        let mut p = plan(purpose, JobKind::Check, command.clone(), &dir, &c.project).rw(dir);
        p.unwalled = true;
        p.files.insert("tmp/.keep".into(), String::new());
        Ok(p)
    }

    pub(super) fn wall_runs_tick(&mut self) {
        let ended: Vec<_> = self
            .pipe
            .slots
            .walls
            .keys()
            .filter_map(|lease| {
                self.state
                    .jobs
                    .values()
                    .find(|j| {
                        j.settled.is_some()
                            && matches!(&j.purpose, Some(Purpose::WallTests { lease: id, .. }) if id == lease)
                    })
                    .map(|job| (lease.clone(), self.wall_run_output(job)))
            })
            .collect();
        for (lease, answer) in ended {
            self.pipe.slots.held.remove(&lease);
            if let Some((conn, req)) = self.pipe.slots.walls.remove(&lease) {
                conn.answer(req, Ok(json!({"walls": answer})));
            }
        }
    }

    fn wall_run_output(&self, job: &Job) -> Value {
        let dir = self.job_dir(&job.id);
        let exit = dir.exit().ok().flatten();
        let stdout = std::fs::read_to_string(dir.out()).unwrap_or_default();
        let mut stderr = std::fs::read_to_string(dir.err()).unwrap_or_default();
        let code = if job.stop_requested || exit.as_ref().is_some_and(|e| e.stopped) {
            3
        } else {
            exit.as_ref().and_then(|e| e.code.or_else(|| e.signal.map(|s| 128 + s))).unwrap_or(2)
        };
        if let Some(why) = exit.as_ref().and_then(|e| e.error.as_deref()) {
            stderr.push_str(&format!("{why}\n"));
        }
        if let Some(Settlement::Failed { words }) = &job.settled {
            stderr.push_str(&format!("{words}\n"));
        }
        if exit.is_none() && stderr.is_empty() {
            stderr.push_str("the wall-test job ended without an exit; run it again\n");
        }
        json!({"exit": code, "stdout": stdout, "stderr": stderr})
    }

    pub(super) fn wall_runs_end(&mut self) {
        let leases: Vec<_> = self
            .state
            .leases
            .values()
            .filter(|l| l.kind == factory_types::lease::LeaseKind::Walls)
            .map(|l| l.id.clone())
            .collect();
        for lease in leases {
            self.apply(&Event::LeaseClosed { lease });
        }
    }
}
