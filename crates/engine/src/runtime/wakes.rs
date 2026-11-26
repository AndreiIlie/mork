// `wakes.json` keeps pending wakes across reloads (fac-340), including batches a waiter may still need (fac-663).

mod waiting;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde::Deserialize;

use mork_core::state::Change;
use mork_types::card::{Card, Mode};
use mork_types::{CardId, JobId, ProjectId};

use super::{Engine, Phase, upkeep};
use crate::jobs::{context, waker};
use crate::paths::Home;
use crate::{clock, files, random};
use waiting::{Batch, Waiter};

// Lines one run carries, at most.
const PER_RUN: usize = 50;

/// What the wakes hold: each project's queued lines, its run in flight.
#[derive(Default)]
pub struct Wakes {
    queues: BTreeMap<ProjectId, VecDeque<String>>,
    // Cards that ended, their lines held until they settle.
    ending: Vec<CardId>,
    running: BTreeMap<ProjectId, Running>,
    waiting: BTreeMap<ProjectId, Waiter>,
    delivering: BTreeMap<ProjectId, Batch>,
    cut: Arc<waker::Cut>,
    no_login_said: bool,
}

// A run in flight and the lines it carries, which a reload's cut puts back.
struct Running {
    rx: Receiver<Result<(), String>>,
    job: JobId,
    lines: Vec<String>,
}

// What `wakes.json` holds.
#[derive(Default, Deserialize)]
struct Kept {
    queues: BTreeMap<ProjectId, VecDeque<String>>,
    ending: Vec<CardId>,
}

impl Wakes {
    /// The wakes as `wakes.json` kept them; a missing file is none, an unreadable one is logged.
    pub fn open(home: &Home) -> Self {
        let kept = files::read_json::<Kept>(&home.wakes()).unwrap_or_else(|e| {
            upkeep::log(home, &format!("the kept wake lines couldn't be read: {e}"));
            None
        });
        let Kept { queues, ending } = kept.unwrap_or_default();
        Self { queues, ending, ..Self::default() }
    }
}

impl Engine {
    // Writes the queues and the ended cards after a change to either.
    fn wakes_keep(&self) {
        let kept = serde_json::json!({ "queues": &self.wakes.queues, "ending": &self.wakes.ending });
        if let Err(e) = files::write_json(&self.opts.home.wakes(), &kept) {
            self.log(&format!("the wake lines couldn't be kept: {e}"));
        }
    }

    // The cards these changes end: live before, done or closed after. Read before the state commits them. A review
    // card's end wakes nobody (fac-330).
    pub(super) fn cards_ending(&self, changes: &[Change]) -> Vec<Card> {
        changes
            .iter()
            .filter_map(|c| match c {
                Change::PutCard(card) if !card.stage.is_live() && card.mode != Mode::Review => Some(card),
                _ => None,
            })
            .filter(|card| self.state.cards.get(&card.id).is_some_and(|was| was.stage.is_live()))
            .map(|card| (**card).clone())
            .collect()
    }

    pub(super) fn wakes_note(&mut self, changes: &[Change], ended: &[Card]) {
        let events: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                Change::AddOrchestratorEvent(e) => {
                    Some((e.project.clone(), mork_core::messages::wakes::line(&self.state, &e.what)?))
                }
                _ => None,
            })
            .collect();
        for (project, line) in events {
            self.wake_queue(project, line);
        }
        for c in ended {
            // Ended again before it settled: still one line.
            if !self.wakes.ending.contains(&c.id) {
                self.wakes.ending.push(c.id.clone());
                self.wakes_keep();
            }
        }
    }

    // A finished reload's announcement, as main says it, wakes Mork's own orchestrator alone (fac-386).
    pub(super) fn wakes_reloaded(&mut self, text: String) {
        if let Some(project) = self.ctx.config.mork.project.clone() {
            self.wake_queue(project, text);
        }
    }

    fn wake_queue(&mut self, project: ProjectId, line: String) {
        if self
            .ctx
            .config
            .projects
            .get(&project)
            .is_some_and(|p| p.orchestrator_session.is_some() || p.orchestrator_waits)
        {
            self.wakes.queues.entry(project).or_default().push_back(line);
            self.wakes_keep();
        }
    }

    // Each ended card that settled is queued, saying what it left; one live again since, or gone, never is.
    fn wakes_settle(&mut self) {
        // One kept across a reload may not have loaded (fac-336): read before it is judged.
        let unloaded = &self.state.unloaded.cards;
        let cards = self.wakes.ending.iter().filter(|c| unloaded.contains(*c)).cloned().collect();
        self.load(&mork_core::state::unloaded::Wanted { cards, ..Default::default() });
        let ending = std::mem::take(&mut self.wakes.ending);
        let before = ending.clone();
        for id in ending {
            let Some(card) = self.state.cards.get(&id).filter(|c| !c.stage.is_live()) else { continue };
            if !mork_core::messages::wakes::settled(&self.state, card) {
                self.wakes.ending.push(id);
                continue;
            }
            let mork = self.ctx.config.mork.project.as_ref();
            let line = mork_core::messages::wakes::ended(&self.state, card, mork);
            let project = card.project.clone();
            self.wake_queue(project, line);
        }
        if self.wakes.ending != before {
            self.wakes_keep();
        }
    }

    pub(super) fn wakes_tick(&mut self) {
        self.wakes_settle();
        self.tick_waiting_wakes();
        let sessions: Vec<(ProjectId, String)> = self
            .ctx
            .config
            .projects
            .iter()
            .filter_map(|(p, c)| Some((p.clone(), c.orchestrator_session.clone()?)))
            .collect();
        for (project, session) in sessions {
            if let Some(run) = self.wakes.running.get(&project) {
                match run.rx.try_recv() {
                    Ok(result) => {
                        self.wakes.running.remove(&project);
                        if let Err(e) = result {
                            self.log(&format!("waking {project}'s orchestrator failed: {e}"));
                        }
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.wakes.running.remove(&project);
                    }
                    Err(TryRecvError::Empty) => continue,
                }
            }
            self.wake_start(&project, &session);
        }
    }

    // Starts the haiku on the project's queued lines, at most a run's worth.
    fn wake_start(&mut self, project: &ProjectId, session: &str) {
        if self.wakes.queues.get(project).is_none_or(VecDeque::is_empty) {
            return;
        }
        if !matches!(self.phase, Phase::Ready) || context::wall_unavailable(self.ctx.mode, &self.ctx.bwrap).is_some() {
            return;
        }
        if !waker::login(&self.ctx).exists() {
            // Dropped like a failed run's: no retry, and the queue doesn't grow while there is no login.
            self.wakes.queues.remove(project);
            self.wakes_keep();
            if !std::mem::replace(&mut self.wakes.no_login_said, true) {
                let home = self.ctx.home.waker_home().display().to_string();
                self.log(&format!("the orchestrators' waker has no login: run HOME={home} claude and /login once"));
            }
            return;
        }
        self.wakes.no_login_said = false;
        let now = clock::now_ms();
        let Some(queue) = self.wakes.queues.get_mut(project) else { return };
        let lines: Vec<String> = queue.drain(..PER_RUN.min(queue.len())).collect();
        self.wakes_keep();
        let id = mork_core::state::names::job_id(now, &mork_types::AgentKey::new(waker::OWNER), random::salt());
        let spec = match waker::spec(&self.ctx, &id, now) {
            Ok(spec) => spec,
            Err(e) => {
                self.log(&format!("the orchestrators' waker couldn't start: {e}"));
                return;
            }
        };
        let (root, prompt) = (self.opts.home.root().to_path_buf(), waker::prompt(session, &lines));
        let (for_run, carried) = (project.clone(), lines.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let cut = Arc::clone(&self.wakes.cut);
        std::thread::spawn(move || {
            let _ = tx.send(waker::run(&spec, &root, &for_run, &lines, &prompt, &cut));
        });
        self.wakes.running.insert(project.clone(), Running { rx, job: id, lines: carried });
    }

    // A clean shutdown ends the runs in flight: no process of one is left.
    pub(super) fn end_wakes(&mut self) {
        self.settle_waiting_wakes();
        self.wakes.cut.stop.store(true, Ordering::SeqCst);
    }

    // A reload never waits for a run (fac-340): each in flight is cut, kept in `wakes.log` as the reload's, and its
    // lines go back at the front of its queue for the next engine. One that answered or failed first is not sent again.
    pub(super) fn wakes_cut_for_reload(&mut self) {
        self.wakes.cut.reload.store(true, Ordering::SeqCst);
        self.end_wakes();
        let root = self.opts.home.root().to_path_buf();
        for (project, run) in std::mem::take(&mut self.wakes.running) {
            // One the stop already ended has logged its cut itself, and is queued again all the same.
            match run.rx.try_recv() {
                Ok(Err(e)) if e == waker::RELOAD_CUT => {}
                Ok(_) => continue,
                Err(_) => waker::cut_by_reload(&root, &project, &run.job, &run.lines),
            }
            let queue = self.wakes.queues.entry(project).or_default();
            run.lines.into_iter().rev().for_each(|l| queue.push_front(l));
        }
        self.wakes_keep();
    }

    // The exec failed and this engine carries on: its runs are no longer cut.
    pub(super) fn wakes_resume(&mut self) {
        self.wakes.cut.stop.store(false, Ordering::SeqCst);
        self.wakes.cut.reload.store(false, Ordering::SeqCst);
    }
}
