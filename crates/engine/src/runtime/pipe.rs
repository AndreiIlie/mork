// The pipeline's part of the engine's loop (section 4): intents carried out after their commit, the boards read,
// time passing (`Tick`), and the poller's reads of Bitbucket.

use std::time::{Duration, Instant};

use factory_core::decide::{Event, Intent};
use factory_types::CardId;
use factory_types::card::{BoardCard, Mode, Stage};

use super::{Engine, Input};
use crate::bitbucket::calls;
use crate::{board, clock, faults};

/// What the pipeline keeps in memory.
#[derive(Debug, Default)]
pub struct Pipe {
    /// Intents decided, carried out at the next turn of the loop (never inside a decision).
    pub later: Vec<Intent>,
    pub locks: super::actions::Locks,
    /// The Azure tickets the open PRs refer to, as the poller last read them (fac-292).
    pub tickets: crate::azure::Cache,
    /// Bitbucket's short hashes made full, kept across polls (fac-361).
    pub fulls: calls::Fulls,
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
    pub recovered: Vec<(factory_types::JobId, bool)>,
    /// The secrets the app's log masks, read again a minute after the last read (3.8.3).
    pub masks: std::sync::Mutex<Option<(Instant, crate::secrets::Secrets)>>,
    /// The token's identity was asked of Bitbucket since this start (3.7.2, M3.2): until then,
    /// actions that reach Bitbucket wait here.
    pub token_asked: bool,
    pub token_wait: Vec<factory_types::ActionId>,
    /// The token's verdict, as the action threads read it before each call (M4.5).
    pub verdict: super::verdict::Verdict,
    /// Actions whose call is on its thread, until their answer is applied: a reload waits for
    /// them, up to 10 s (1.8.6.3, M5.17).
    pub in_flight: std::collections::BTreeSet<factory_types::ActionId>,
    /// Clone jobs whose thread reads have not returned yet (fac-448).
    pub review_threads: std::collections::BTreeSet<factory_types::JobId>,
    pub(super) work: super::work_threads::Workers,
}

/// A board's `config.yml`, in its `backlog/` (4.1).
pub fn board_config(board: &str) -> std::path::PathBuf {
    std::path::Path::new(board).join("backlog/config.yml")
}

/// Reads every board's cards (4.18): `tasks/`, `completed/`, `archive/`; and each board's id
/// prefix, from its `config.yml` (`task_prefix`, M5.27), none for one whose config doesn't parse.
pub fn read_boards(
    config: &factory_types::config::Config,
) -> (Vec<BoardCard>, std::collections::BTreeMap<factory_types::ProjectId, String>) {
    let mut out = Vec::new();
    let mut prefixes = std::collections::BTreeMap::new();
    for (name, p) in &config.projects {
        let b = std::path::Path::new(&p.board).join("backlog");
        let parsed = std::fs::read_to_string(board_config(&p.board)).ok().and_then(|t| board::config(&t).ok());
        if let Some((prefix, _)) = &parsed {
            prefixes.insert(name.clone(), prefix.clone());
        }
        let (prefix, done) = parsed.unwrap_or_default();
        for dir in ["tasks", "completed", "archive/tasks"] {
            for e in std::fs::read_dir(b.join(dir)).into_iter().flatten().flatten() {
                let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
                let Ok(f) = board::parse(&text) else { continue };
                // Its whole text and its board's prefix: what `core` makes a write from (M3.16).
                out.push(BoardCard {
                    id: CardId::new(f.id.to_lowercase()),
                    project: name.clone(),
                    title: f.title,
                    description: f.description,
                    acceptance: f.acceptance,
                    status: f.status,
                    labels: f.labels,
                    done_status: done.clone(),
                    file: format!("{dir}/{}", e.file_name().to_string_lossy()),
                    text,
                    prefix: prefix.clone(),
                });
            }
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
            self.apply(&Event::Tick);
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
        let folders: Vec<(factory_types::ProjectId, String)> = self
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
        faults::hit("notice.before-send");
        let secrets = crate::secrets::Secrets::gather(&self.opts.home, &self.ctx.config, &self.ctx.telemetry);
        let title = format!("factory · {}", a.place);
        let body = secrets.mask(&a.text);
        let home = self.opts.home.root().to_path_buf();
        std::thread::spawn(move || {
            let _ = crate::jobs::launch::short(&home, &["notify-send", &title, &body], &home, Duration::from_secs(10));
        });
    }

    fn poll(&mut self) {
        let now = clock::now_ms();
        let every = i64::try_from(self.ctx.config.factory.poll_seconds).unwrap_or(60).saturating_mul(1000);
        if self.pipe.last_poll.is_some_and(|t| now < t + every)
            || self.pipe.polling.load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        self.pipe.last_poll = Some(now);
        let reads: Vec<(CardId, factory_types::ProjectId, u64)> = self
            .state
            .cards
            .values()
            .filter(|c| polled(c, &self.state))
            .filter_map(|c| c.pr.map(|pr| (c.id.clone(), c.project.clone(), pr)))
            .collect();
        let owned = factory_core::queue::approval::owned(&self.state);
        let projects: Vec<(factory_types::ProjectId, factory_types::config::ProjectConfig)> = self
            .ctx
            .config
            .projects
            .iter()
            .filter(|(_, p)| p.bitbucket.is_some())
            .map(|(n, p)| (n.clone(), p.clone()))
            .collect();
        if projects.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let home = self.opts.home.clone();
        let locks = self.pipe.locks.clone();
        let tickets = self.pipe.tickets.clone();
        let fulls = self.pipe.fulls.clone();
        let verdict = self.pipe.verdict.clone();
        let flag = std::sync::Arc::clone(&self.pipe.polling);
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        std::thread::spawn(move || {
            poll_now(&locks, &tx, &home, &projects, (&reads, &owned, &tickets, &fulls), &verdict);
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
        let fulls = self.pipe.fulls.clone();
        std::thread::spawn(move || {
            if let Some(e) = pr_event((&locks, &fulls), &home, &card, &c.project, &p, pr, before_merge) {
                let _ = tx.send(Input::Event(Box::new(e)));
            }
        });
    }
}

fn pr_event(
    (locks, fulls): (&super::actions::Locks, &calls::Fulls),
    home: &crate::paths::Home,
    card: &CardId,
    project: &str,
    p: &factory_types::config::ProjectConfig,
    pr: u64,
    before_merge: bool,
) -> Option<Event> {
    let repo = super::act_bitbucket::repo(p).ok()?;
    let approver = super::act_bitbucket::approver().unwrap_or_default();
    let (mut seen, merge) = calls::read_pr(&repo, pr, &approver, fulls).ok()?;
    if let Some(hash) = merge {
        // Read under the project's lock, the mirror being its actions' too (R19). Nothing is fetched here: a fetch
        // is an action, which `core` decides when it needs one (2.5, M4.24).
        let lock = locks.of(project);
        let _held = lock.lock();
        let m = crate::git::mirror::ensure(home, project).ok()?;
        if let Some(full) = crate::git::mirror::rev(home, &m, &hash) {
            seen.merge_parents =
                crate::git::mirror::parents(home, &m, &full).into_iter().map(factory_types::Sha::new).collect();
            seen.merge_commit = Some(factory_types::Sha::new(full));
        }
    }
    Some(Event::PrRead { card: card.clone(), seen, before_merge })
}

// The cards' PRs the poll reads, the PRs a live card of ours holds (fac-98), the tickets read for
// the open PRs, and the full hashes kept.
type Reads<'a> = (
    &'a [(CardId, factory_types::ProjectId, u64)],
    &'a [(factory_types::ProjectId, u64)],
    &'a crate::azure::Cache,
    &'a calls::Fulls,
);

fn poll_now(
    locks: &super::actions::Locks,
    tx: &std::sync::mpsc::Sender<Input>,
    home: &crate::paths::Home,
    projects: &[(factory_types::ProjectId, factory_types::config::ProjectConfig)],
    reads: Reads<'_>,
    verdict: &super::verdict::Verdict,
) {
    let text = crate::token::read_text();
    under_read(&text, || poll_projects(locks, tx, home, projects, reads, verdict, &text));
}

// The poll's `GET /user`, its verdict and its reads are all of the one token text it read (M5.11).
fn under_read<T>(text: &Result<String, crate::token::TokenError>, f: impl FnOnce() -> T) -> T {
    match text {
        Ok(t) => crate::token::pinned(t.clone(), f),
        Err(_) => f(),
    }
}

fn poll_projects(
    locks: &super::actions::Locks,
    tx: &std::sync::mpsc::Sender<Input>,
    home: &crate::paths::Home,
    projects: &[(factory_types::ProjectId, factory_types::config::ProjectConfig)],
    (reads, owned, tickets, fulls): Reads<'_>,
    verdict: &super::verdict::Verdict,
    text: &Result<String, crate::token::TokenError>,
) {
    let send = |e: Event| {
        let _ = tx.send(Input::Event(Box::new(e)));
    };
    let token = text.as_ref().map_err(|_| ()).and_then(|t| crate::token::parse(t).map_err(|_| ()));
    let mut identity_read = false;
    for (name, p) in projects {
        let Ok(repo) = super::act_bitbucket::repo(p) else { continue };
        if !identity_read {
            identity_read = true;
            // `GET /user` that gives nothing usable accepts it (3.7.2): no PR opens with it to tell
            // its author, every one opening in Andrei's name (fac-111).
            match calls::identity(&repo, token.as_ref().ok().and_then(|t| t.approver.as_deref())) {
                Ok(refused) => {
                    if let Ok(t) = &text {
                        verdict.asked(t, refused.clone());
                    }
                    send(Event::TokenRead { refused })
                }
                Err(crate::bitbucket::client::BitbucketError::Answered(a)) if a.status == 403 => {
                    if let Ok(t) = &text {
                        verdict.unsaid(t);
                    }
                    send(Event::TokenAccepted)
                }
                Err(_) => {}
            }
            let _ = tx.send(Input::TokenAsked);
        }
        let me = token.as_ref().ok().and_then(|t| t.user.clone());
        if let Ok(prs) = calls::open_prs(&repo, me.as_deref(), fulls) {
            let prs = tickets.attach(name, p, prs);
            open_read((locks, fulls), home, (name, p), prs, owned, &send);
        }
        for (card, _, pr) in reads.iter().filter(|(_, proj, _)| proj == name) {
            if let Some(e) = pr_event((locks, fulls), home, card, name, p, *pr, false) {
                send(e);
            }
        }
    }
    // No repository could be asked (the token unreadable): its calls fail on their own, held.
    if !identity_read {
        let _ = tx.send(Input::TokenAsked);
    }
}

// Each PR the approval detector watches, into its repository's base, but one a live card of ours
// holds, is read as its review card's after the list, on the first poll too (fac-98, fac-111).
fn open_read(
    kept: (&super::actions::Locks, &calls::Fulls),
    home: &crate::paths::Home,
    (name, p): (&factory_types::ProjectId, &factory_types::config::ProjectConfig),
    prs: Vec<factory_types::work::OpenPr>,
    owned: &[(factory_types::ProjectId, u64)],
    send: &impl Fn(Event),
) {
    let watched: Vec<u64> = prs
        .iter()
        .filter(|x| factory_core::reviews::watch::watched(p, &x.dest) && !owned.contains(&(name.clone(), x.pr)))
        .map(|x| x.pr)
        .collect();
    send(Event::OpenPrsRead { project: name.clone(), prs });
    for n in watched {
        let card = factory_core::reviews::cards::card_id(name, n);
        if let Some(e) = pr_event(kept, home, &card, name, p, n, false) {
            send(e);
        }
    }
}

// A live full card's PR is read whatever its stage or alarm, so a merge lands it (fac-171). A PR
// Andrei asked to review and merge is read for his approval, whatever it merges into (fac-111).
fn polled(c: &factory_types::card::Card, state: &factory_core::state::State) -> bool {
    let asked = c.review.as_ref().is_some_and(|r| r.merge);
    let open = state.world.open_prs.get(&c.project).is_some_and(|prs| prs.iter().any(|pr| c.pr == Some(pr.pr)));
    (c.stage.is_live()
        && (c.mode == Mode::Full || ((matches!(c.stage, Stage::Ready | Stage::Merging) || asked) && !c.alarm)))
        || (c.stage == Stage::Done && asked && open)
}
