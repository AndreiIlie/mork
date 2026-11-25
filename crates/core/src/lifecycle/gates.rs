//! Heads and their gates (4.2.3, 4.6, 4.7, 4.10): a head taken after the foreman's turn, the checks on it, and
//! the card ready, its verification starting once they pass (fac-652).

use factory_types::Sha;
use factory_types::action::ActionWhat;
use factory_types::ask::{AskKind, Color};
use factory_types::card::{Card, Mode, Stage};
use factory_types::reply::Role;
use factory_types::work::{CheckRun, CloneStatus, Purpose, Taken};
use factory_types::{CardId, JobId};
use serde_json::Value;

use crate::decide::Ctx;
use crate::messages::asks::{self, Raise};
use crate::messages::requests::{ask_agent, tell_agent};
use crate::post::Poster;

pub fn has_checks(card: &Card, ctx: Ctx<'_>) -> bool {
    ctx.config.projects.get(&card.project).is_some_and(|p| p.checks.is_some())
}

/// Accepts the latest bundle, taking a new head or refreshing its ancestry before submission.
/// A `built` with no committed change asks for a commit.
pub fn bundle_ended(p: &mut Poster<'_>, mut card: Card, job: &JobId, s: CloneStatus, ctx: Ctx<'_>) {
    // A bundle the run asked for or recorded since reads its clone later: its end takes the head,
    // and answers the `built` (fac-97). One of a withdrawn submission takes none (fac-591).
    let later = card.bundled.as_ref().and_then(|b| b.job.as_ref()).is_some_and(|j| j != job);
    if bundle_owed(p, &card.id) || later || card.retired.contains(job) {
        p.journal("bundle.superseded", Some(card.id.as_str()), serde_json::json!({"job": job, "head": s.head}));
        return;
    }
    let read = s.head.clone().filter(|_| s.bundle);
    if let Some(b) = card.bundled.as_mut().filter(|b| b.job.as_ref() == Some(job)) {
        b.head.clone_from(&read);
    }
    card.gates.clean = s.clean;
    let moved = read.as_ref().is_some_and(|h| card.head.as_ref() != Some(h));
    let submit = card.gates.built_pending && card.gates.clean && !late_head(&card);
    let id = card.id.clone();
    p.put_card(card.clone());
    // A closed or landed card's last bundle: its head taken if new, then its clone removed (R8, fac-171).
    if ended(&card) && !moved {
        p.act(ActionWhat::RemoveClone { card: id, run: card.run, keep_proofs: false });
        return;
    }
    if let Some(head) = read.filter(|_| moved || submit) {
        p.act_as(ActionWhat::Take { card: id, head, job: job.clone() }, true);
        return;
    }
    if card.gates.built_pending {
        built(p, card, false, None, ctx);
    }
}

fn late_head(card: &Card) -> bool {
    matches!(card.stage, Stage::Ready | Stage::Merging | Stage::Done)
}

// A dirty tree gets a request to commit, which costs no round; the head past its gates stays where it
// is (fac-97).
fn built(p: &mut Poster<'_>, mut card: Card, moved: bool, tip: Option<Sha>, ctx: Ctx<'_>) {
    card.gates.built_pending = false;
    crate::reviews::triage::withdraw(p, &mut card);
    let id = card.id.clone();
    if !card.gates.clean {
        p.put_card(card);
        let text = "Your `built` left uncommitted changes. Commit what belongs to the change, remove the rest, then answer `built` again.";
        ask_agent(p, &id, Role::Foreman, "Commit your changes", text);
        return;
    }
    if card.head.is_none() || (tip.is_none() && !late_head(&card)) {
        p.put_card(card);
        ask_agent(
            p,
            &id,
            Role::Foreman,
            "Nothing committed",
            "Nothing is committed past the base yet. Commit your change, then answer `built`.",
        );
        return;
    }
    if !moved && late_head(&card) {
        return late(p, card);
    }
    if card.mode == Mode::Full
        && let Some(tip) = tip
        && card.fork.as_ref() != Some(&tip)
    {
        card.gates.checking = None;
        card.gates.slot_wait = None;
        let why = crate::queue::joins::BEFORE_CHECKS;
        return crate::queue::joins::through_foreman(p, card, why, tip, ctx);
    }
    start_checks(p, card, ctx);
}

// A late `built` of the same head: its stage, gates and train candidate stay, the foreman told why.
fn late(p: &mut Poster<'_>, card: Card) {
    let id = card.id.clone();
    let head = card.head.as_ref().map(|h| h.short().to_owned()).unwrap_or_default();
    let pr = card.pr;
    let at = match card.stage {
        Stage::Merging => "verifying or in the merge queue",
        Stage::Done => "landed",
        Stage::Ready
        | Stage::Starting
        | Stage::Brief
        | Stage::Build
        | Stage::Checks
        | Stage::Review
        | Stage::Second
        | Stage::Report
        | Stage::Closed => "through its gates",
    };
    p.journal("built.late", Some(id.as_str()), serde_json::json!({"head": card.head, "stage": card.stage}));
    p.put_card(card);
    let rerun = if pr.is_some() {
        format!(" `factory ctl rereview {id}` runs its PR's failed run again on that head.")
    } else {
        String::new()
    };
    let text = format!(
        "Your `built` found the clone at {head}, the head already {at}: nothing changes, and what it is in goes on. A new commit goes through the gates again.{rerun}"
    );
    tell_agent(p, &id, Role::Foreman, &format!("{head} is already {at}"), &text);
}

/// Accepts a take only if no bundle is owed and it belongs to the latest bundle (fac-97).
pub fn take_returned(p: &mut Poster<'_>, card: Card, head: &Sha, job: &JobId, t: Taken, ctx: Ctx<'_>) {
    if card.retired.contains(job) {
        return super::retired::take_returned(p, &card, head, job);
    }
    let fresh = !bundle_owed(p, &card.id) && card.bundled.as_ref().is_some_and(|b| last_bundle(b, job, head));
    if fresh {
        taken(p, card, t, ctx);
    } else {
        p.journal("take.superseded", Some(card.id.as_str()), serde_json::json!({"head": head, "job": job}));
    }
}

/// Whether a take is of the card's last bundle, the one whose head it keeps (fac-97).
pub(crate) fn last_bundle(b: &factory_types::card::Bundled, job: &JobId, head: &Sha) -> bool {
    b.job.as_ref() == Some(job) && b.head.as_ref() == Some(head)
}

// A bundle of the card asked for and not yet recorded, which reads its clone after every one
// recorded: its request is the latest submission's (fac-97).
fn bundle_owed(p: &Poster<'_>, id: &CardId) -> bool {
    let of = |w: &Purpose| matches!(w, Purpose::Bundle { card, refresh: None, .. } if card == id);
    let asked = p.other.iter().any(|c| matches!(c, crate::state::Change::PutOwedWork(w) if of(w)));
    asked || p.state.owed_work.values().any(of)
}

/// A job recorded: a bundle of a card's is its run's latest, whose end alone takes a head (fac-97).
pub fn bundle_recorded(state: &crate::state::State, job: &factory_types::state::Job, d: &mut crate::decide::Decision) {
    // A refresh reads no head (fac-333).
    let Some(Purpose::Bundle { card, refresh: None, .. }) = &job.purpose else { return };
    let Some(mut c) = state.cards.get(card).cloned() else { return };
    c.bundled = Some(factory_types::card::Bundled { job: Some(job.id.clone()), head: None });
    d.changes.push(crate::state::Change::PutCard(Box::new(c)));
}

/// Whether a card's run is over, closed or landed: a bundle of it only keeps its head, then its
/// clone goes.
pub fn ended(c: &Card) -> bool {
    matches!(c.stage, Stage::Closed | Stage::Done)
}

// A head taken (the `take` action's result).
fn taken(p: &mut Poster<'_>, mut card: Card, t: Taken, ctx: Ctx<'_>) {
    let id = card.id.clone();
    if ended(&card) {
        card.head = Some(t.head);
        let run = card.run;
        p.put_card(card);
        p.act(ActionWhat::RemoveClone { card: id, run, keep_proofs: false });
        return;
    }
    if t.rewrites_pushed {
        card.gates.built_pending = false;
        crate::reviews::triage::withdraw(p, &mut card);
        p.put_card(card);
        let text = format!(
            "Head {} rewrites commits already pushed, so it isn't taken. Add commits on top of the pushed ones instead.",
            t.head
        );
        ask_agent(p, &id, Role::Foreman, "Add commits on top", &text);
        return;
    }
    let same_round = card.stage == Stage::Merging
        && crate::queue::order::candidate(&card).and_then(|k| k.commit.as_ref()) == Some(&t.head)
        && crate::reviews::end::keep_open_submitted(p, &card, &t.head);
    let moved = card.head.as_ref() != Some(&t.head) && !same_round;
    // Its code changed while merging: it leaves for its gates (fac-10, decision 10).
    if moved && card.stage == Stage::Merging && card.landing.is_some() {
        card = crate::queue::members::left_for_head(p, card, &t.head, ctx);
    }
    if moved {
        // The round under way reviews what it no longer lands (fac-95); any open triage is stale.
        crate::reviews::triage::withdraw(p, &mut card);
        crate::reviews::end::moved(p, &card, &t.head);
        let built = card.gates.built_pending;
        let clean = card.gates.clean;
        card.gates = factory_types::card::Gates { built_pending: built, clean, ..Default::default() };
    }
    // A new head at `ready` goes through the gates again: what waited on the old one goes.
    if moved && card.stage == Stage::Ready {
        asks::close(p, id.as_str(), &[AskKind::ApprovePr]);
    }
    // The base tip stays the one the clone holds: the mirror's may never have reached it (fac-12).
    card.head = Some(t.head.clone());
    card.fork = Some(t.fork.clone());
    card.changed = t.changed.clone();
    card.contents = t.contents.clone();
    card.lines = t.lines;
    p.put_card(card.clone());
    super::overlaps::compare(p, &id, ctx);
    let card = p.card(&id).unwrap_or(card);
    if card.gates.built_pending {
        built(p, card, moved, Some(t.base_tip), ctx);
    } else if moved && matches!(card.stage, Stage::Checks | Stage::Ready) {
        start_checks(p, card, ctx);
    }
}

/// The checks passed on the card's head, with their folder at the commit it is now (4.7.7, R13);
/// none to pass with no checks.
pub fn checks_stand(p: &Poster<'_>, card: &Card, ctx: Ctx<'_>) -> bool {
    if !has_checks(card, ctx) {
        return true;
    }
    let Some((head, then)) = &card.gates.checked else { return false };
    let now = p.state.world.checks_folders.get(&card.project);
    card.head.as_ref() == Some(head) && now.is_none_or(|now| now == then)
}

/// Runs the head's checks, then its verification; with no checks, its verification at once (fac-652).
pub fn start_checks(p: &mut Poster<'_>, mut card: Card, ctx: Ctx<'_>) {
    let Some(head) = card.head.clone() else { return };
    card.gates.checks_waiting = None;
    if !has_checks(&card, ctx) {
        return passed(p, card, ctx);
    }
    card.stage = Stage::Checks;
    card.gates.checking = Some(head.clone());
    let fork = card.fork.clone().unwrap_or_else(|| head.clone());
    let id = card.id.clone();
    p.put_card(card);
    asks::close(p, id.as_str(), &[AskKind::Checks]);
    crate::queue::leases::start_job(p, Purpose::Check { card: id, head, fork }, ctx);
}

/// Why a checks run can't be trusted (4.7.5), or its findings' count.
pub(crate) fn verdict(run: &CheckRun) -> Result<usize, String> {
    if run.folder_dirty {
        return Err("the checks folder has uncommitted changes".into());
    }
    if run.folder_moved {
        return Err("the checks folder moved while they ran".into());
    }
    report_verdict(run.exit, run.report.as_ref(), &run.located)
}

/// Why a checks run's exit and report can't be trusted (4.7.5), or its findings' count: the gate's,
/// and `factory test --checks`'s (fac-165). `located` is each finding's place in the clone.
pub fn report_verdict(
    exit: Option<i32>,
    report: Option<&Value>,
    located: &[Result<String, String>],
) -> Result<usize, String> {
    let Some(report) = report else { return Err("they wrote no report".into()) };
    let status = report.get("status").and_then(Value::as_i64);
    let findings = report.get("findings").and_then(Value::as_array).map_or(0, Vec::len);
    match exit {
        Some(code) if status != Some(i64::from(code)) => Err(format!("their status isn't their exit ({code})")),
        Some(0) if findings > 0 => Err("exit 0 naming findings".into()),
        Some(0) => Ok(0),
        Some(1) if findings == 0 => Err("exit 1 naming nothing".into()),
        Some(1) => match located.iter().find_map(|l| l.as_ref().err()) {
            Some(why) => Err(format!("a finding outside the clone: {why}")),
            None => Ok(findings),
        },
        Some(code) => Err(format!("exit {code}")),
        None => Err("they were stopped".into()),
    }
}

/// A checks run ended (4.7.3–4.7.6).
pub fn check_ended(p: &mut Poster<'_>, mut card: Card, run: CheckRun, ctx: Ctx<'_>) {
    // Stale: the new head is checked instead.
    if card.head.as_ref() != Some(&run.head) || card.gates.checking.as_ref() != Some(&run.head) {
        return;
    }
    card.gates.checking = None;
    let id = card.id.clone();
    let output = run.report.as_ref().map(|r| r.to_string()).unwrap_or_default();
    match verdict(&run) {
        Ok(0) => {
            card.gates.checked = Some((run.head.clone(), run.folder_commit.clone()));
            card.checks_output = Some(output);
            passed(p, card, ctx);
        }
        Ok(n) => {
            p.put_card(card.clone());
            let text = format!("Fix the checks' findings below ({n})\n\n{}", super::fixes::findings_text(&run));
            super::fixes::send_back(p, card, &text);
        }
        Err(why) => {
            card.gates.checks_waiting = Some(run.head.clone());
            p.put_card(card);
            let text = format!("the checks on {} can't be trusted: {why}; /resume runs them again", run.head.short());
            asks::raise(p, Raise::new(id.as_str(), AskKind::Checks, Color::Red, text));
        }
    }
}

// Its head passed its gates, its checks or none configured: an own card's verification starts (fac-652).
pub(crate) fn passed(p: &mut Poster<'_>, mut card: Card, ctx: Ctx<'_>) {
    card.gates.passed.clone_from(&card.head);
    train(p, card, ctx);
}

/// The card is ready (4.10): an own card's verification starts at once, no `/merge` waited for (fac-68,
/// decision 3), then its merge queue; on Bitbucket it publishes once its verification passed (fac-375).
pub(super) fn train(p: &mut Poster<'_>, mut card: Card, ctx: Ctx<'_>) {
    let head = card.head.clone();
    card.stage = Stage::Ready;
    p.put_card(card.clone());
    // A colleague's PR never reaches here: its review is its own (FAC-23).
    let (Some(_), Mode::Full) = (head, card.mode) else { return };
    crate::queue::members::enter(p, card, ctx);
}
