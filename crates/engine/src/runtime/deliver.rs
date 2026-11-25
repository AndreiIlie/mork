// Delivering (fac-389): a message handed to a running turn goes into its folder, its images into its `$FACTORY_IO`,
// for its supervisor to speak into the turn's stdin.

use std::path::Path;

use factory_types::JobId;

use super::Engine;
use crate::cli::live::{Delivery, message_uuid};
use crate::jobs::folder;

impl Engine {
    /// Writes message `seq` into running job `job`'s input, each image copied from its source to its name in the
    /// turn's `$FACTORY_IO`.
    pub(super) fn deliver(&mut self, job: &JobId, seq: u64, text: &str, images: &[(String, String)]) {
        let dir = self.job_dir(job);
        let io = self.state.jobs.get(job).map_or_else(|| dir.io(), |j| dir.factory_io(&j.spec));
        let mut shown = Vec::new();
        for (rel, src) in images {
            // Written beneath its `io/` through no link: the turn may have planted one there.
            let copied = super::turn_images::read_image(&self.opts.home, src).and_then(|bytes| {
                factory_sys::mkdir_beneath(&io, Path::new("images"))?;
                factory_sys::write_beneath(&io, Path::new(rel), &bytes)
            });
            match copied {
                Ok(()) => shown.push(io.join(rel).display().to_string()),
                Err(e) => self.log(&format!("{job}: message {seq}'s image {src} couldn't be copied: {e}")),
            }
        }
        let mut text = text.to_owned();
        if !shown.is_empty() {
            text.push_str("\n\nAttached images; view each with Read:\n");
            text.push_str(&shown.join("\n"));
        }
        let delivery = Delivery { uuid: message_uuid(job.as_str(), seq), text, images: shown };
        if let Err(e) = folder::deliver(&dir, seq, &delivery) {
            self.log(&format!("{job}: message {seq} couldn't be delivered: {e}"));
        }
    }
}
