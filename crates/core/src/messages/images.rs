//! A message's images (1.4.11.5, fac-278, fac-654): an agent's are the copies the engine published from its `io/`
//! folder, `$FACTORY_IO`, for its `say` as it was sent or for its turn's reply.

use factory_types::job::Mount;
use factory_types::reply::Reply;
use factory_types::state::{Job, Sender};

use crate::state::State;

/// An agent's images must be the copies the engine published as it sent them, in a running job's `published/`: its
/// `$FACTORY_IO` changes later (fac-654), and a path outside its wall would leak a file (1.4.11.5). Andrei's needn't.
pub(super) fn allowed(state: &State, from: &Sender, images: &[String]) -> Result<(), String> {
    let Sender::Agent(key) = from else { return Ok(()) };
    let jobs = state.jobs.values().filter(|j| &j.owner == key && j.settled.is_none());
    let published: Vec<String> = jobs.filter_map(published_dir).collect();
    images.iter().try_for_each(|i| {
        if !published.iter().any(|dir| plain_in(dir, i)) {
            return Err(format!("the image {i} isn't in your io/ folder, $FACTORY_IO: an agent's images go there"));
        }
        Ok(())
    })
}

/// A reply's images (fac-278) must be the copies the engine published for this job: it replaces
/// every path the agent wrote with its copy or with why it made none, which this refuses.
pub fn reply_allowed(job: &Job, reply: &Reply) -> Result<(), String> {
    let published = published_dir(job);
    let hand = reply.hand.images.iter();
    hand.chain(reply.messages.iter().flat_map(|m| m.images.iter())).try_for_each(|i| {
        if !published.as_ref().is_some_and(|dir| plain_in(dir, i)) {
            return Err(format!(
                "the image {i} isn't a regular file of your io/ folder, $FACTORY_IO: name it by its full path there, \
                 reached through no link"
            ));
        }
        Ok(())
    })
}

// Its `published/`, in the home its wall's `io` lies in: no folder an agent writes, so a copy there stays as made.
fn published_dir(j: &Job) -> Option<String> {
    let home = io_dir(j).and_then(|io| home_of(j, io))?;
    Some(format!("{home}/jobs/{}/published/", j.id))
}

// A plain name alone, so no `..` nor sub-folder reaches past the copies.
fn plain_in(dir: &str, image: &str) -> bool {
    image.strip_prefix(dir).is_some_and(|n| !n.is_empty() && !n.contains('/') && n != "." && n != "..")
}

// The job's `$FACTORY_IO`, when its wall binds that very folder read-write.
fn io_dir(j: &Job) -> Option<&str> {
    let io = j.spec.env.get("FACTORY_IO")?.trim_end_matches('/');
    let bound = j.spec.mounts.iter().any(|m| matches!(m, Mount::Rw { path } if path.trim_end_matches('/') == io));
    (bound && !io.is_empty()).then_some(io)
}

// The home its `io` lies in: `<home>/io/<seat>` for a seat's turn (fac-654), `<home>/jobs/<job>/io` for any other job.
fn home_of<'a>(j: &Job, io: &'a str) -> Option<&'a str> {
    if let Some(home) = io.strip_suffix(&format!("/jobs/{}/io", j.id)) {
        return Some(home);
    }
    let (folder, seat) = io.rsplit_once('/')?;
    let plain = !seat.is_empty() && seat != "." && seat != "..";
    folder.strip_suffix("/io").filter(|_| plain)
}
