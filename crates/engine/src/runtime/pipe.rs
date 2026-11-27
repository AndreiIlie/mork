// The pipeline's part of the engine's loop (section 4): intents carried out after their commit, the boards read,
// time passing (`Tick`), and the poller's reads of Bitbucket.

use std::time::{Duration, Instant};

use mork_core::decide::{Event, Intent};
use mork_types::CardId;
use mork_types::card::{BoardCard, Mode, Stage};

use super::{Engine, Input};
use crate::board::BoardRead;
use crate::forge::{self, Identity, Readers};
use crate::lock::Lock;
use crate::{board, clock};

/// What the pipeline keeps in memory.
#[derive(Debug, Default)]
pub struct Pipe {
    /// Intents decided, carried out at the next turn of the loop (never inside a decision).
    pub later: Vec<Intent>,
    pub locks: super::actions::Locks,
    /// The Azure tickets the open PRs refer to, as the poller last read them (fac-292).
    pub tickets: crate::tickets::Cache,
    /// Bitbucket's short hashes made full, kept across polls (fac-361).
    pub(super) readers: Readers,
    pub last_boards: Option<Instant>,
    pub last_tick: Option<Instant>,
    /// The poller's last read, on the (test) clock.
    pub last_poll: Option<i64>,
    /// A poll under way on its thread.
    pub polling: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Leases and the connections that hold them; `test.lease` calls that wait (4.9).
    pub slots: super::slots::Slots,
    /// Launches recovery decided, started once it has ended: the job, and whether its folder is
    /// written first.
    pub recovered: Vec<(mork_types::JobId, bool)>,
    /// The secrets the app's log masks, read again a minute after the last read (3.8.3).
    pub(super) masks: Lock<Option<(Instant, crate::secrets::Secrets)>>,
    /// The token's identity was asked of Bitbucket since this start (3.7.2, M3.2): until then,
    /// actions that reach Bitbucket wait here.
    pub token_asked: std::collections::BTreeSet<mork_types::ProjectId>,
    pub token_wait: std::collections::BTreeMap<mork_types::ProjectId, Vec<mork_types::ActionId>>,
    /// The token's verdict, as the action threads read it before each call (M4.5).
    pub verdict: super::verdict::Verdicts,
    /// Actions whose call is on its thread, until their answer is applied: a reload waits for
    /// them, up to 10 s (1.8.6.3, M5.17).
    pub in_flight: std::collections::BTreeSet<mork_types::ActionId>,
    /// Clone jobs whose thread reads have not returned yet (fac-448).
    pub review_threads: std::collections::BTreeSet<mork_types::JobId>,
    pub(super) work: super::work_threads::Workers,
}

/// Reads every board's cards (4.18): `tasks/`, `completed/`, `archive/`; and each board's id
/// prefix, from its `config.yml` (`task_prefix`, M5.27), none for one whose config doesn't parse.
pub fn read_boards(
    config: &mork_types::config::Config,
) -> (Vec<BoardCard>, std::collections::BTreeMap<mork_types::ProjectId, String>) {
    let mut out = Vec::new();
    let mut prefixes = std::collections::BTreeMap::new();
    for (name, p) in &config.projects {
        let (cards, prefix) = board::read(&p.board).snapshot(name);
        out.extend(cards);
        if let Some(prefix) = prefix {
            prefixes.insert(name.clone(), prefix);
        }
    }
    (out, prefixes)
}

impl Engine {
    /// An intent, after its decision committed.
    pub(super) fn carry_out_later(&mut self, intent: Intent) {
        match intent {
            Intent::StopJob { id } => self.stop_job(&id),
            Intent::Deliver { job, seq, text, images } => self.deliver(&job, seq, &text, &images),
            Intent::StartJob { purpose } => self.start_work(&purpose),
            Intent::RunAction { id } => self.run_action(&id),
            Intent::Notify { ask } => self.notify(&ask),
            Intent::WriteRecord { card } => self.write_record(&card),
            Intent::ReadPr { card } => self.read_pr(&card, true),
            Intent::SlotFreed => self.pipe.slots.freed = true,
            Intent::CopyProofs { review, card, round } => self.copy_proofs(&review, &card, round),
            Intent::RemoveHandoff { project, dir } => self.remove_handoff(&project, &dir),
            Intent::DropMock { card } => self.drop_kept_mock(&card),
        }
    }

    /// Carries out what earlier decisions asked, in order.
    pub(super) fn drain_later(&mut self) {
        for _ in 0..50 {
            if self.pipe.later.is_empty() {
                return;
            }
            let batch = std::mem::take(&mut self.pipe.later);
            for i in batch {
                self.carry_out_later(i);
            }
        }
    }

    /// The pipeline's share of each turn of the loop.
    pub(super) fn pipe_tick(&mut self) {
        self.drain_later();
        self.door_tick();
        if self.pipe.last_boards.is_none_or(|t| clock::elapsed(t) > Duration::from_secs(2)) {
            self.pipe.last_boards = Some(clock::instant());
            self.read_boards_now();
        }
        if self.pipe.last_tick.is_none_or(|t| clock::elapsed(t) > Duration::from_secs(1)) {
            self.pipe.last_tick = Some(clock::instant());
            let claude_account = self.views_cache.claude_accounts.live.as_ref().map(|a| a.id.clone());
            self.apply(&Event::Tick { claude_account });
        }
        self.poll();
        self.slots_tick();
        self.drain_later();
    }

    /// The boards read again; `core` gets them when they changed.
    pub(super) fn read_boards_now(&mut self) {
        let (cards, prefixes) = read_boards(&self.ctx.config);
        let same = cards.len() == self.state.board.len()
            && cards.iter().all(|c| self.state.board.get(&c.id) == Some(c))
            && prefixes == self.state.world.board_prefixes;
        if !same {
            self.apply(&Event::BoardsRead { cards, prefixes });
        }
        self.read_checks_folders();
    }

    fn read_checks_folders(&mut self) {
        let folders: Vec<(mork_types::ProjectId, String)> = self
            .ctx
            .config
            .projects
            .iter()
            .filter_map(|(n, p)| p.checks.as_ref().map(|k| (n.clone(), k.folder.clone())))
            .collect();
        for (project, folder) in folders {
            let Some(commit) = crate::git::head_of(std::path::Path::new(&folder)) else { continue };
            if self.state.world.checks_folders.get(&project) != Some(&commit) {
                self.apply(&Event::ChecksFolderSeen { project, commit });
            }
        }
    }

    // At most once: claimed in its decision (2.6).
    fn notify(&mut self, ask: &str) {
        let Some(a) = self.state.asks.get(ask).cloned() else { return };
        let secrets = crate::secrets::Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        let title = format!("mork · {}", a.place);
        let body = secrets.mask(&a.text);
        let home = self.opts.home.root().to_path_buf();
        std::thread::spawn(move || {
            let _ = crate::jobs::launch::short(&home, &["notify-send", &title, &body], &home, Duration::from_secs(10));
        });
    }

    fn poll(&mut self) {
        let now = clock::now_ms();
        let every = i64::try_from(self.ctx.config.mork.poll_seconds).unwrap_or(60).saturating_mul(1000);
        if self.pipe.last_poll.is_some_and(|t| now < t + every)
            || self.pipe.polling.load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        self.pipe.last_poll = Some(now);
        let reads: Vec<(CardId, mork_types::ProjectId, u64)> = self
            .state
            .cards
            .values()
            .filter(|c| polled(c, &self.state))
            .filter_map(|c| c.pr.map(|pr| (c.id.clone(), c.project.clone(), pr)))
            .collect();
        let owned = mork_core::queue::approval::owned(&self.state);
        let projects: Vec<(mork_types::ProjectId, mork_types::config::ProjectConfig)> = self
            .ctx
            .config
            .projects
            .iter()
            .filter(|(_, p)| p.forge.hosted())
            .map(|(n, p)| (n.clone(), p.clone()))
            .collect();
        if projects.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let home = self.opts.home.clone();
        let locks = self.pipe.locks.clone();
        let tickets = self.pipe.tickets.clone();
        let readers = self.pipe.readers.clone();
        let verdict = self.pipe.verdict.clone();
        let flag = std::sync::Arc::clone(&self.pipe.polling);
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        std::thread::spawn(move || {
            poll_now(&locks, &tx, &home, &projects, (&reads, &owned, &tickets, &readers), &verdict);
            flag.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn read_pr(&mut self, card: &CardId, before_merge: bool) {
        let Some(c) = self.state.cards.get(card).cloned() else { return };
        let (Some(pr), Ok(p)) = (c.pr, self.project(&c.project)) else { return };
        let tx = self.tx.clone();
        let card = card.clone();
        let home = self.opts.home.clone();
        let locks = self.pipe.locks.clone();
        let readers = self.pipe.readers.clone();
        std::thread::spawn(move || {
            if let Some(e) = pr_event((&locks, &readers), &home, &card, &c.project, &p, pr, before_merge) {
                let _ = tx.send(Input::Event(Box::new(e)));
            }
        });
    }
}

fn pr_event(
    (locks, readers): (&super::actions::Locks, &Readers),
    home: &crate::paths::Home,
    card: &CardId,
    project: &str,
    p: &mork_types::config::ProjectConfig,
    pr: u64,
    before_merge: bool,
) -> Option<Event> {
    let repo = readers.read(&p.forge).ok()?;
    let mut seen = repo.observe(pr).ok()?;
    if let Some(hash) = &seen.merge_commit {
        // Read under the project's lock, the mirror being its actions' too (R19). Nothing is fetched here: a fetch
        // is an action, which `core` decides when it needs one (2.5, M4.24).
        let lock = locks.of(project);
        let _held = lock.lock();
        let m = crate::git::mirror::ensure(home, project).ok()?;
        if let Some(full) = crate::git::mirror::rev(home, &m, hash.as_str()) {
            seen.merge_parents =
                crate::git::mirror::parents(home, &m, &full).into_iter().map(mork_types::Sha::new).collect();
            seen.merge_commit = Some(mork_types::Sha::new(full));
        }
    }
    Some(Event::PrRead { card: card.clone(), seen, before_merge })
}

// The cards' PRs the poll reads, the PRs a live card of ours holds (fac-98), the tickets read for
// the open PRs, and the full hashes kept.
type Reads<'a> = (
    &'a [(CardId, mork_types::ProjectId, u64)],
    &'a [(mork_types::ProjectId, u64)],
    &'a crate::tickets::Cache,
    &'a Readers,
);

fn poll_now(
    locks: &super::actions::Locks,
    tx: &std::sync::mpsc::Sender<Input>,
    home: &crate::paths::Home,
    projects: &[(mork_types::ProjectId, mork_types::config::ProjectConfig)],
    reads: Reads<'_>,
    verdicts: &super::verdict::Verdicts,
) {
    let (reads, owned, tickets, readers) = reads;
    let send = |e: Event| {
        let _ = tx.send(Input::Event(Box::new(e)));
    };
    for (name, p) in projects {
        let asked = || {
            let _ = tx.send(Input::TokenAsked(name.clone()));
        };
        let lock = locks.of(name);
        let held = lock.lock();
        let credential = match forge::Credential::read(&p.forge) {
            Ok(c) => c,
            Err(mork_types::work::ActionFailure::Token { words }) => {
                send(Event::TokenRead { project: name.clone(), refused: Some(words) });
                asked();
                continue;
            }
            Err(_) => {
                asked();
                continue;
            }
        };
        credential.pinned(|| {
            let Ok(repo) = readers.read(&p.forge) else {
                asked();
                return;
            };
            let verdict = verdicts.of(name);
            match repo.identity() {
                Ok(Identity::Known(refused)) => {
                    verdict.asked(credential.text(), refused.clone());
                    send(Event::TokenRead { project: name.clone(), refused });
                }
                Ok(Identity::Unknown) => {
                    verdict.unsaid(credential.text());
                    send(Event::TokenRead { project: name.clone(), refused: None });
                }
                Err(_) => {}
            }
            asked();
            drop(held);
            if let Ok(prs) = repo.open_prs() {
                let prs = tickets.attach(name, p, prs);
                open_read((locks, readers), home, (name, p), prs, owned, &send);
            }
            for (card, _, pr) in reads.iter().filter(|(_, proj, _)| proj == name) {
                if let Some(e) = pr_event((locks, readers), home, card, name, p, *pr, false) {
                    send(e);
                }
            }
        });
    }
}

// Each PR the approval detector watches, into its repository's base, but one a live card of ours
// holds, is read as its review card's after the list, on the first poll too (fac-98, fac-111).
fn open_read(
    kept: (&super::actions::Locks, &Readers),
    home: &crate::paths::Home,
    (name, p): (&mork_types::ProjectId, &mork_types::config::ProjectConfig),
    prs: Vec<mork_types::work::OpenPr>,
    owned: &[(mork_types::ProjectId, u64)],
    send: &impl Fn(Event),
) {
    let watched: Vec<u64> = prs
        .iter()
        .filter(|x| !x.fork && mork_core::reviews::watch::watched(p, &x.dest) && !owned.contains(&(name.clone(), x.pr)))
        .map(|x| x.pr)
        .collect();
    send(Event::OpenPrsRead { project: name.clone(), prs });
    for n in watched {
        let card = mork_core::reviews::cards::card_id(name, n);
        if let Some(e) = pr_event(kept, home, &card, name, p, n, false) {
            send(e);
        }
    }
}

// A live full card's PR is read whatever its stage or alarm, so a merge lands it (fac-171). A PR
// The operator asked to review and merge is read for his approval, whatever it merges into (fac-111).
fn polled(c: &mork_types::card::Card, state: &mork_core::state::State) -> bool {
    let asked = c.review.as_ref().is_some_and(|r| r.merge);
    let open = state.world.open_prs.get(&c.project).is_some_and(|prs| prs.iter().any(|pr| c.pr == Some(pr.pr)));
    (c.stage.is_live()
        && (c.mode == Mode::Full || ((matches!(c.stage, Stage::Ready | Stage::Merging) || asked) && !c.alarm)))
        || (c.stage == Stage::Done && asked && open)
}
