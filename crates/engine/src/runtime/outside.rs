// Reads of the world beyond the state (section 5), made now and then on the tick: the pin against master (1.8.9),
// master's tip (fac-19) and each live card's base ahead of its head; the rest each view needs is read for that view.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use factory_core::views::outside::{AgentSeen, LookbackCounts, Outside};
use factory_types::lookback::Read;
use factory_types::view::{Master, Phase as ViewPhase, Resources};
use factory_types::{CardId, ProjectId, Sha};

use super::{Engine, Phase};
use crate::{builds, clock, git};

// How often the git reads behind the views run: never per view.
const EVERY: Duration = Duration::from_secs(20);
const GIT: Duration = Duration::from_secs(10);
const STEPS_EVERY: Duration = Duration::from_millis(250);

/// The reads of the world kept between ticks.
#[derive(Debug, Default)]
pub struct Cache {
    pub(crate) claude_accounts: crate::claude_accounts::Accounts,
    pub tz_offset_min: i32,
    pub master: Master,
    /// The factory repo's master tip, as Andrei's checkout has it, when git can say.
    pub tip: Option<String>,
    pub base_ahead: BTreeMap<CardId, u32>,
    pub lookback_counts: LookbackCounts,
    /// The machine's latest sample, never stored.
    pub resources: Option<Resources>,
    pub read: Option<Instant>,
    /// A read is under way on its thread.
    pub busy: bool,
    /// An agent's steps moved since the views were last sent, and when they were.
    pub stale: bool,
    pub sent: Option<Instant>,
    pub(super) chat: std::cell::RefCell<super::chat::Cache>,
}

/// One round of the reads, made off the engine's thread.
#[derive(Debug, Default)]
pub struct Reads {
    tz_offset_min: i32,
    master: Master,
    tip: Option<String>,
    base_ahead: BTreeMap<CardId, u32>,
    lookback_counts: LookbackCounts,
}

impl Engine {
    /// The reads behind the views, again when they are old, on a thread of their own: git and
    /// `date` never hold the engine's loop (1.5). Their result comes back as an input.
    pub(super) fn refresh_outside(&mut self) {
        if self.views_cache.claude_accounts.refresh(&self.opts.home.agent_home()) {
            self.align_claude_readings();
            self.publish_screen();
        }
        if self.views_cache.busy || self.views_cache.read.is_some_and(|t| clock::elapsed(t) < EVERY) {
            return;
        }
        self.views_cache.read = Some(clock::instant());
        self.views_cache.busy = true;
        let (home, config, tx) = (self.opts.home.clone(), self.ctx.config.clone(), self.tx.clone());
        let heads: Vec<(CardId, String, ProjectId)> = self
            .state
            .cards
            .values()
            .filter(|c| c.stage.is_live())
            .filter_map(|c| c.head.as_ref().map(|h| (c.id.clone(), h.0.clone(), c.project.clone())))
            .collect();
        let cursors: Vec<_> = self
            .state
            .lookbacks
            .values()
            .flat_map(|reads| reads.values())
            .filter_map(|c| c.commit.clone().map(|commit| (c.project.clone(), c.read, commit)))
            .collect();
        std::thread::spawn(move || {
            let r = Reads {
                tz_offset_min: tz_offset(&home),
                master: master(&home, &config),
                tip: tip(&home, &config),
                base_ahead: bases_ahead(&home, &config, &heads),
                lookback_counts: lookback_counts(&home, &config, cursors),
            };
            let _ = tx.send(super::Input::Outside(Box::new(r)));
        });
    }

    /// The views again for the steps that moved, at most every quarter second.
    pub(super) fn publish_steps(&mut self) {
        if self.views_cache.stale && self.views_cache.sent.is_none_or(|t| clock::elapsed(t) >= STEPS_EVERY) {
            self.publish();
        }
    }

    /// The reads came back: kept, and every watcher's views sent again (times, pin, bases).
    pub(super) fn outside_read(&mut self, r: Reads) {
        let c = &mut self.views_cache;
        c.busy = false;
        c.tz_offset_min = r.tz_offset_min;
        c.master = r.master;
        c.tip = r.tip;
        c.base_ahead = r.base_ahead;
        c.lookback_counts = r.lookback_counts;
        self.master_seen();
        self.publish();
    }

    /// A sample of the machine's load came back: shown at once, to the screen's watchers alone.
    pub(super) fn resources_read(&mut self, r: Resources) {
        if self.views_cache.resources.as_ref() != Some(&r) {
            self.views_cache.resources = Some(r);
            self.publish_screen();
        }
    }

    /// What a view needs of the world; a card's timeline and cost only for `card`.
    pub(super) fn outside(&self, card: Option<&CardId>) -> Outside {
        let now = clock::now_ms();
        let c = &self.views_cache;
        let mut o = Outside {
            tz_offset_min: c.tz_offset_min,
            pin: self.pin_line(c.master),
            phase: match &self.phase {
                Phase::Ready => ViewPhase::Ready,
                Phase::Reloading { target, .. } => ViewPhase::Reloading { short: target.chars().take(7).collect() },
            },
            usage: super::usage::shares(&self.state, now, &c.claude_accounts),
            base_ahead: c.base_ahead.clone(),
            lookback_counts: c.lookback_counts.clone(),
            resources: c.resources.clone(),
            suites: self.suite_counts(),
            ..Outside::default()
        };
        for (key, seen) in &self.seen {
            // Shown when both used and effective window are known (5.7.4, fac-681).
            let context = seen
                .context
                .as_ref()
                .and_then(|x| self.ctx.config.factory.effective_window(x.window).map(|w| (x.used, w)));
            let (steps, doing, turn) = (seen.steps.clone(), seen.doing.clone(), seen.turn.clone());
            o.agents.insert(key.clone(), AgentSeen { steps, doing, context, turn, list: seen.list.clone() });
        }
        for p in &self.pipe.slots.parked {
            if let factory_types::state::Sender::Agent(k) = &p.from {
                o.slot_waits.insert(k.clone());
            }
        }
        if let Some(id) = card {
            self.card_outside(&mut o, id);
        }
        o
    }

    fn card_outside(&self, o: &mut Outside, id: &CardId) {
        let rows = self.store.card_events(id.as_str()).unwrap_or_default();
        let from = rows.iter().rposition(|(_, _, k, _)| k == "card.start").unwrap_or(0);
        let marks: Vec<(i64, &str, &serde_json::Value)> =
            rows[from..].iter().map(|(t, _, k, d)| (*t, k.as_str(), d)).collect();
        let (points, cost) = factory_core::views::timeline::points(&marks);
        o.timelines.insert(id.clone(), points);
        o.card_costs.insert(id.clone(), cost.div_ceil(10_000));
        o.seat_costs.insert(id.clone(), factory_core::views::timeline::seat_costs(&marks));
        // A turn that reported no cost is journaled as unknown, its share counted as nothing.
        if marks
            .iter()
            .any(|(_, k, d)| *k == "cost" && d.get("unknown").and_then(serde_json::Value::as_bool) == Some(true))
        {
            o.card_costs_unknown.insert(id.clone());
        }
        let turns = self.state.jobs.values().filter(|j| {
            j.kind == factory_types::job::JobKind::Turn && j.owner.as_str().split('/').next() == Some(id.as_str())
        });
        o.outputs.insert(id.clone(), crate::jobs::outputs::produced(&self.opts.home, turns));
        let clone = self.opts.home.clones().join(id.as_str());
        if clone.exists() {
            o.clones.insert(id.clone(), clone.display().to_string());
        }
    }
}

fn tz_offset(home: &crate::paths::Home) -> i32 {
    let out = crate::jobs::launch::short(home.root(), &["date", "+%z"], home.root(), GIT);
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits = text.trim_start_matches(['+', '-']);
    let (h, m) =
        (digits.get(..2).and_then(|h| h.parse::<i32>().ok()), digits.get(2..4).and_then(|m| m.parse::<i32>().ok()));
    h.zip(m).map_or(0, |(h, m)| sign * (h * 60 + m))
}

// Andrei's checkout is never fetched (1.8.9); unknown when Git can't say.
fn master(home: &crate::paths::Home, config: &factory_types::config::Config) -> Master {
    let (Some(pin), Some(project)) = (builds::pin(home), config.projects.get(&config.factory.project)) else {
        return Master::Unknown;
    };
    let range = format!("{pin}..{}", project.base);
    let out = git::git(home, Path::new(&project.repo), &["rev-list", "--count", &range], GIT);
    match out.ok().and_then(|n| n.trim().parse::<u32>().ok()) {
        Some(0) => Master::Equal,
        Some(commits) => Master::Ahead { commits },
        None => Master::Unknown,
    }
}

// Read as `master` is: Andrei's checkout, never fetched.
fn tip(home: &crate::paths::Home, config: &factory_types::config::Config) -> Option<String> {
    let project = config.projects.get(&config.factory.project)?;
    let base = &project.base;
    let spec =
        if base.starts_with("refs/") { format!("{base}^{{commit}}") } else { format!("refs/heads/{base}^{{commit}}") };
    let out = git::git(home, Path::new(&project.repo), &["rev-parse", "--verify", "-q", &spec], GIT).ok()?;
    let full = out.trim().to_owned();
    (full.len() == 40).then_some(full)
}

fn bases_ahead(
    home: &crate::paths::Home,
    config: &factory_types::config::Config,
    heads: &[(CardId, String, ProjectId)],
) -> BTreeMap<CardId, u32> {
    let mut out = BTreeMap::new();
    for (id, head, project) in heads {
        let Some(p) = config.projects.get(project) else { continue };
        let mirror = home.mirrors().join(project.as_str());
        let range = format!("{head}..refs/heads/{}", p.base);
        let n = git::git(home, &mirror, &["rev-list", "--count", &range], GIT).ok();
        if let Some(n) = n.and_then(|n| n.trim().parse::<u32>().ok()) {
            out.insert(id.clone(), n);
        }
    }
    out
}

fn lookback_counts(
    home: &crate::paths::Home,
    config: &factory_types::config::Config,
    cursors: Vec<(ProjectId, Read, Sha)>,
) -> LookbackCounts {
    cursors
        .into_iter()
        .filter_map(|(project, read, commit)| {
            let p = config.projects.get(&project)?;
            let repo =
                if p.bitbucket.is_some() { home.mirrors().join(project.as_str()) } else { p.repo.clone().into() };
            let range = format!("{commit}..refs/heads/{}", p.base);
            let n = git::git(home, &repo, &["rev-list", "--count", "--end-of-options", &range, "--"], GIT).ok()?;
            Some(((project, read), (commit, n.trim().parse().ok()?)))
        })
        .collect()
}
