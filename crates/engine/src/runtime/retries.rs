// A retry sent short (fac-626): the session it resumes holds its request word for word, so it is asked to go
// on with it rather than sent it again; anything less proven sends the whole prompt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use factory_core::agents::turns::TurnPlan;
use factory_types::JobId;
use factory_types::config::Cli;

use super::Engine;
use crate::cli;
use crate::jobs::folder;

impl Engine {
    /// A retry's short prompt and the sections it stages, when the session it resumes holds its request's prompt
    /// word for word; the images it carries are named where this turn has them.
    pub(super) fn short_retry(
        &self,
        plan: &TurnPlan,
        cli: Cli,
        env: &BTreeMap<String, String>,
        cwd: &str,
        shown: &[String],
    ) -> Option<(String, BTreeMap<String, String>)> {
        let (short, of) = (plan.short.as_ref()?, plan.request.as_ref()?);
        let session = plan.session.as_ref().filter(|_| plan.resume)?;
        let held = &self.state.jobs.get(of)?.stdin;
        // Said by job and reason alone: a session's text never reaches the log.
        if let Err(why) = cli::history::holds(cli, env, cwd, session.as_str(), held) {
            self.log(&format!("{}: its retry of {of} is sent whole: {why}", plan.agent));
            return None;
        }
        self.log(&format!("{}: its retry of {of} is sent short, its session holding that prompt", plan.agent));
        let (mut prompt, staged) = short.clone();
        if !shown.is_empty() {
            prompt.push_str("\n\nThat request's images are now at these paths; view each with Read:\n");
            prompt.push_str(&shown.join("\n"));
            prompt.push('\n');
        }
        Some((prompt, staged))
    }

    // A retry takes the images of the try `carry` names under the same names (M2.8), from their sources again when
    // it ran in this folder (fac-654), else from its; the folder its text names, its opening request's or that try's.
    pub(super) fn turn_images(
        &self,
        plan: &TurnPlan,
        id: &JobId,
        io: &Path,
    ) -> (BTreeMap<String, String>, Vec<String>, Option<PathBuf>) {
        let (mut copies, mut shown) = super::turn_images::images_of(&plan.images, id.as_str(), io);
        let Some(old) = plan.carry.as_ref().and_then(|old| self.state.jobs.get(old)) else {
            return (copies, shown, None);
        };
        let old_io = self.io_of(&old.id);
        for (rel, src) in &old.io_copies {
            let from = if old_io == io { src.clone() } else { old_io.join(rel).display().to_string() };
            copies.insert(rel.clone(), from);
            shown.push(io.join(rel).display().to_string());
        }
        (copies, shown, Some(self.io_of(plan.request.as_ref().unwrap_or(&old.id))))
    }

    // The folder job `id`'s `FACTORY_IO` named: its seat's for a turn (fac-654), else its own `io/`.
    fn io_of(&self, id: &JobId) -> PathBuf {
        let dir = folder::JobDir::new(self.opts.home.job(id.as_str()));
        self.state.jobs.get(id).map_or_else(|| dir.io(), |j| dir.factory_io(&j.spec))
    }
}
