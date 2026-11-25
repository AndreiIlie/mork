// An agent's `say` carries copies of its images, made as it is sent (fac-654): its seat's `io/` changes in its later
// turns, so the message's delivery, its retries and the window never read that folder again.

use std::collections::HashMap;
use std::path::PathBuf;

use factory_core::decide::Event;
use factory_types::protocol::{ErrorBody, ErrorCode};
use factory_types::state::Sender;
use factory_types::{Command, JobId};
use serde_json::{Value, json};

use super::Engine;
use crate::jobs::published;

impl Engine {
    /// A command `core` decides, an agent's `say` images replaced first with copies in its calling job's `published/`,
    /// removed again when it is refused.
    pub(super) fn decided(&mut self, mut cmd: Command, from: Sender, job: Option<&JobId>) -> Result<Value, ErrorBody> {
        let copies = self.say_copies(&mut cmd, &from, job)?;
        let answer = self.apply(&Event::Command { from, cmd }).answer.unwrap_or_else(|| Ok(json!({})));
        if answer.is_err() {
            remove(&copies);
        }
        answer
    }

    // Each image named once is copied once, read beneath the calling job's `$FACTORY_IO`; one that can't be refuses
    // the whole message, its copies made so far removed.
    fn say_copies(&self, cmd: &mut Command, from: &Sender, job: Option<&JobId>) -> Result<Vec<PathBuf>, ErrorBody> {
        let (Command::Say { images: Some(images), .. }, Sender::Agent(key)) = (cmd, from) else {
            return Ok(Vec::new());
        };
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let refused = |m: String| ErrorBody::new(ErrorCode::Refused, m);
        let running = job.and_then(|id| self.state.jobs.get(id)).filter(|j| &j.owner == key && j.settled.is_none());
        let Some(j) = running else { return Err(refused("an agent's images are sent from its running turn".into())) };
        let io = self.job_dir(&j.id).factory_io(&j.spec);
        let jobs = self.opts.home.jobs();
        let mut made: HashMap<String, String> = HashMap::new();
        for image in images.iter_mut() {
            if let Some(copy) = made.get(image.as_str()) {
                *image = copy.clone();
                continue;
            }
            match published::say_copy(&jobs, j.id.as_str(), &io, image) {
                Ok(copy) => {
                    made.insert(image.clone(), copy.clone());
                    *image = copy;
                }
                Err(e) => {
                    remove(&made.into_values().map(PathBuf::from).collect::<Vec<_>>());
                    let words = "an agent's images are regular files in its io/ folder, $FACTORY_IO";
                    return Err(refused(format!("the image {image} can't be sent ({e}): {words}")));
                }
            }
        }
        Ok(made.into_values().map(PathBuf::from).collect())
    }
}

// Copies the engine made in a job's own `published/`, outside every wall.
fn remove(copies: &[PathBuf]) {
    for copy in copies {
        let _ = std::fs::remove_file(copy);
    }
}
