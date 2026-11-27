//! The state `core` decides on and the changes it makes (1.12.1), which the engine loads at start and applies as they
//! commit; what it leaves in the store unloaded, and the names made from it.

pub mod names;
pub mod unloaded;

use std::collections::BTreeMap;

use mork_types::CardId;
use mork_types::action::Action;
use mork_types::ask::Ask;
use mork_types::card::{BoardCard, Card};
use mork_types::lease::{Lease, Pass};
use mork_types::state::{Agent, Job, Message, MessageKind, Place};
use mork_types::usage::Limit;
use mork_types::{AgentKey, JobId};
use serde::{Deserialize, Serialize};

/// Everything `core` decides on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub revision: u64,
    pub agents: BTreeMap<AgentKey, Agent>,
    pub places: BTreeMap<mork_types::PlaceId, Place>,
    pub jobs: BTreeMap<JobId, Job>,
    pub messages: Vec<Message>,
    pub next_seq: u64,
    pub cards: BTreeMap<CardId, Card>,
    pub notes: BTreeMap<u64, mork_types::note::Note>,
    /// The last allocated note id survives deletion, so triage cannot delete a later note by an old id.
    pub notes_last: u64,
    /// Asks by id, open and answered.
    pub asks: BTreeMap<mork_types::AskId, Ask>,
    /// The latest read of each usage-limit window, by [`Limit::key`].
    pub limits: BTreeMap<String, Limit>,
    /// Outward actions, by id (2.5).
    pub actions: BTreeMap<mork_types::ActionId, Action>,
    /// Test leases, by id (4.9), and the suite passes remembered (4.9.6).
    pub leases: BTreeMap<mork_types::LeaseId, Lease>,
    pub passes: Vec<Pass>,
    /// Jobs of no agent's turn owed and not yet recorded, by purpose key (2.2.2, M3.9): stored in the transaction that
    /// owes them, so a crash before the job is recorded loses none.
    pub owed_work: BTreeMap<String, mork_types::work::Purpose>,
    /// The orchestrators' streams' next line's number, one sequence for all projects (fac-119, fac-244):
    /// its lines are stored, never loaded.
    pub orchestrator_next: u64,
    /// Each project's auto switch, by project, on or off (fac-233).
    pub autos: BTreeMap<mork_types::ProjectId, mork_types::orchestrator::Auto>,
    pub landings: BTreeMap<mork_types::ProjectId, u64>,
    pub baselines: BTreeMap<(mork_types::ProjectId, mork_types::lookback::Read), mork_types::lookback::Baseline>,
    pub lookbacks: BTreeMap<mork_types::ProjectId, BTreeMap<mork_types::lookback::Read, mork_types::lookback::Cursor>>,
    /// The memories' files, by [`mork_types::memory::MemoryFile::key`] (fac-159).
    pub memory_files: BTreeMap<String, mork_types::memory::MemoryFile>,
    /// Each memory's cumulative counts, by its name.
    pub memory_tallies: BTreeMap<String, mork_types::memory::Tally>,
    /// The boards' cards as last read: an index, read again from the files, never stored.
    pub board: BTreeMap<CardId, BoardCard>,
    /// What the store holds that wasn't loaded (fac-336): ended cards' records, filled at load.
    pub unloaded: unloaded::Unloaded,
    /// What the engine knows of the world that `core` decides on, and isn't stored: the
    /// token's identity, the open PRs seen first.
    pub world: World,
}

/// The parts of the world `core` decides on that the engine reads again after a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct World {
    /// Read acknowledgements from running job streams, reconstructed when their output is replayed.
    pub live_reads: BTreeMap<JobId, Vec<u64>>,
    /// The token is the operator's own account, or Bitbucket refuses it (3.7.2, 4.11.5): why. Stored
    /// (M3.2), unlike the rest of the world.
    pub token_refused: BTreeMap<mork_types::ProjectId, String>,
    /// The open PRs of each Bitbucket project, as the poller last read them; a project the poller
    /// hasn't looked at yet has none (4.16.1).
    pub open_prs: BTreeMap<mork_types::ProjectId, Vec<mork_types::work::OpenPr>>,
    /// Each project's checks folder, at the commit the engine last read (4.7.7).
    pub checks_folders: BTreeMap<mork_types::ProjectId, String>,
    /// Each board's id prefix, from its `config.yml`: a board with no card yet has one (M5.27).
    pub board_prefixes: BTreeMap<mork_types::ProjectId, String>,
}

/// One change to the state. A transaction is a list of them (2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
// why: a decision's changes are few and short-lived; boxing the agent they mostly carry buys nothing.
#[allow(clippy::large_enum_variant)]
pub enum Change {
    PutAgent(Agent),
    RemoveAgent {
        key: AgentKey,
    },
    PutPlace(Place),
    PutJob(Box<Job>),
    AddMessage(Message),
    PutNote(Box<mork_types::note::Note>),
    RemoveNote {
        id: u64,
    },
    PutCard(Box<Card>),
    PutAsk(Ask),
    PutLimit(Limit),
    PutAction(Box<Action>),
    PutLease(Lease),
    RemoveLease {
        id: mork_types::LeaseId,
    },
    AddPass(Pass),
    /// A line of a project's orchestrator's stream (fac-119, fac-244), stored until its hook has read past it.
    AddOrchestratorEvent(mork_types::orchestrator::OrchestratorEvent),
    /// A project's auto switch (fac-233).
    PutAuto(mork_types::orchestrator::Auto),
    PutBaseline(mork_types::lookback::Baseline),
    PutLandings {
        project: mork_types::ProjectId,
        count: u64,
    },
    PutLookback(mork_types::lookback::Cursor),
    PutMemoryFile(Box<mork_types::memory::MemoryFile>),
    RemoveMemoryFile {
        key: String,
    },
    PutMemoryTally(mork_types::memory::Tally),
    /// A job owed (`StartJob`), stored until it is recorded.
    PutOwedWork(mork_types::work::Purpose),
    RemoveOwedWork {
        key: String,
    },
    /// The boards' cards, read again, and each board's id prefix from its `config.yml` (M5.27):
    /// in memory only, never stored.
    SetBoard {
        cards: Vec<BoardCard>,
        prefixes: BTreeMap<mork_types::ProjectId, String>,
    },
    /// The token's verdict (3.7.2): stored, and read again at each start.
    SetTokenRefused {
        project: mork_types::ProjectId,
        refused: Option<String>,
    },
    /// A project's open PRs, as the poller read them: in memory only.
    SetOpenPrs {
        project: mork_types::ProjectId,
        prs: Vec<mork_types::work::OpenPr>,
    },
    /// A project's checks folder's commit: in memory only, read again at start.
    SetChecksFolder {
        project: mork_types::ProjectId,
        commit: String,
    },
}

impl Change {
    /// Whether the store keeps it: the in-memory parts of the state aren't stored.
    pub fn is_stored(&self) -> bool {
        !matches!(self, Self::SetBoard { .. } | Self::SetOpenPrs { .. } | Self::SetChecksFolder { .. })
    }
}

impl State {
    pub fn apply(&mut self, c: &Change) {
        match c {
            Change::PutAgent(a) => {
                self.agents.insert(a.key.clone(), a.clone());
            }
            Change::RemoveAgent { key } => {
                self.agents.remove(key);
            }
            Change::PutPlace(p) => {
                self.places.insert(p.id.clone(), p.clone());
            }
            Change::PutJob(j) => {
                if j.settled.is_some() {
                    self.world.live_reads.remove(&j.id);
                }
                self.jobs.insert(j.id.clone(), (**j).clone());
            }
            Change::AddMessage(m) => {
                self.next_seq = self.next_seq.max(m.seq + 1);
                self.messages.push(m.clone());
            }
            Change::PutCard(c) => {
                self.cards.insert(c.id.clone(), (**c).clone());
            }
            Change::PutAsk(a) => {
                self.asks.insert(a.id.clone(), a.clone());
            }
            Change::PutLimit(l) => {
                self.limits.insert(l.key(), l.clone());
            }
            Change::PutAction(a) => {
                self.actions.insert(a.id.clone(), (**a).clone());
            }
            Change::PutLease(l) => {
                self.leases.insert(l.id.clone(), l.clone());
            }
            Change::RemoveLease { id } => {
                self.leases.remove(id);
            }
            Change::AddPass(p) => self.passes.push(p.clone()),
            Change::PutNote(_)
            | Change::RemoveNote { .. }
            | Change::PutOwedWork(_)
            | Change::RemoveOwedWork { .. }
            | Change::AddOrchestratorEvent(_)
            | Change::PutAuto(_)
            | Change::PutBaseline(_)
            | Change::PutLandings { .. }
            | Change::PutLookback(_) => self.apply_more(c),
            Change::PutMemoryFile(_) | Change::RemoveMemoryFile { .. } | Change::PutMemoryTally(_) => {
                self.apply_memory(c);
            }
            Change::SetBoard { .. }
            | Change::SetTokenRefused { .. }
            | Change::SetOpenPrs { .. }
            | Change::SetChecksFolder { .. } => self.apply_world(c),
        }
    }

    fn apply_more(&mut self, c: &Change) {
        if let Change::PutOwedWork(w) = c {
            self.owed_work.insert(w.key(), w.clone());
        } else if let Change::RemoveOwedWork { key } = c {
            self.owed_work.remove(key);
        } else if let Change::AddOrchestratorEvent(e) = c {
            self.orchestrator_next = self.orchestrator_next.max(e.seq + 1);
        } else if let Change::PutNote(n) = c {
            self.notes_last = self.notes_last.max(n.id);
            self.notes.insert(n.id, (**n).clone());
        } else if let Change::RemoveNote { id } = c {
            self.notes.remove(id);
        } else if let Change::PutAuto(a) = c {
            self.autos.insert(a.project.clone(), a.clone());
        } else if let Change::PutBaseline(b) = c {
            self.baselines.insert((b.project.clone(), b.read), b.clone());
        } else if let Change::PutLandings { project, count: n } = c {
            self.landings.insert(project.clone(), *n);
        } else if let Change::PutLookback(c) = c {
            self.lookbacks.entry(c.project.clone()).or_default().insert(c.read, c.clone());
        }
    }

    fn apply_memory(&mut self, c: &Change) {
        if let Change::PutMemoryFile(f) = c {
            self.memory_files.insert(f.key(), (**f).clone());
        } else if let Change::RemoveMemoryFile { key } = c {
            self.memory_files.remove(key);
        } else if let Change::PutMemoryTally(t) = c {
            self.memory_tallies.insert(t.memory.clone(), t.clone());
        }
    }

    fn apply_world(&mut self, c: &Change) {
        if let Change::SetBoard { cards, prefixes } = c {
            self.board = cards.iter().map(|c| (c.id.clone(), c.clone())).collect();
            self.world.board_prefixes = prefixes.clone();
        } else if let Change::SetTokenRefused { project, refused: reason } = c {
            if let Some(r) = reason {
                self.world.token_refused.insert(project.clone(), r.clone());
            } else {
                self.world.token_refused.remove(project);
            }
        } else if let Change::SetOpenPrs { project, prs } = c {
            self.world.open_prs.insert(project.clone(), prs.clone());
        } else if let Change::SetChecksFolder { project, commit } = c {
            self.world.checks_folders.insert(project.clone(), commit.clone());
        }
    }

    /// Applies a whole transaction, raising the revision once when it changed anything.
    pub fn commit(&mut self, changes: &[Change]) {
        for c in changes {
            self.apply(c);
        }
        if changes.iter().any(Change::is_stored) {
            self.revision += 1;
        }
    }

    /// The messages of a place, oldest first. A stored `Feedback` message (a build before the
    /// survey posted them) is in none.
    pub fn messages_in<'a>(&'a self, place: &str) -> impl Iterator<Item = &'a Message> {
        self.messages.iter().filter(move |m| m.place == place && m.kind != MessageKind::Feedback)
    }
}
