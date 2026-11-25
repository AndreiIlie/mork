//! A candidate's verification (fac-375): its checks, then its review round and its whole suite at once on the same
//! commit, both results returned together; made again at the merge queue's front, its checks and suite alone.

use factory_types::ask::AskKind;
use factory_types::card::{Candidate, Card, Landing};
use factory_types::landing::HeldRound;
use factory_types::lease::Pass;
use factory_types::suites::SuiteNews;
use factory_types::work::{FinalRun, Purpose};
use factory_types::{CardId, Sha};

use super::candidates::stale;
use super::members::{hold, say};
use super::train::maintain;
use crate::decide::Ctx;
use crate::post::Poster;
use crate::reviews::gate;
use crate::state::Change;

/// Whether a card is a PR's entry, ours or a colleague's.
pub fn pr_entry(c: &Card) -> bool {
    gate::entry(c) && c.pr.is_some()
}

/// Its candidate made: its checks run, unless the candidate is its head and they passed on it.
pub(crate) fn run(p: &mut Poster<'_>, c: Card, k: Candidate, on: Sha, ctx: Ctx<'_>) {
    let Some(commit) = k.commit.clone() else { return };
    let stand = c.head.as_ref() == Some(&commit) && crate::lifecycle::gates::checks_stand(p, &c, ctx);
    if k.checked || stand || !crate::lifecycle::gates::has_checks(&c, ctx) {
        return checked(p, c, k, &on, ctx);
    }
    start(p, c, k, on, true, None, ctx);
}

// A run of its candidate: its checks against `on`, or its suite, walled.
fn start(p: &mut Poster<'_>, c: Card, k: Candidate, on: Sha, checks: bool, test: Option<String>, ctx: Ctx<'_>) {
    let Some(commit) = k.commit.clone() else { return };
    let (card, clone) = (c.id.clone(), k.clone);
    job(p, c, k, Purpose::TrainRun { card, commit, on, clone, checks, test, unwalled: false }, ctx);
}

fn job(p: &mut Poster<'_>, mut c: Card, k: Candidate, purpose: Purpose, ctx: Ctx<'_>) {
    c.landing = Some(Landing::Test(k));
    p.put_card(c);
    // The command it runs is its job's, and its pass is remembered under it (M4.11).
    super::leases::start_job(p, purpose, ctx);
}

/// Its checks passed: at the queue's front, its suite alone while its rounds approve all it has, else it is verified
/// again; verified, its round, unless none is owed, and its suite start at once, neither waiting for the other.
pub(crate) fn checked(p: &mut Poster<'_>, mut c: Card, mut k: Candidate, on: &Sha, ctx: Ctx<'_>) {
    k.checked = true;
    if k.queue {
        if !gate::reviewed(&c) || gate::settles(p, &c, &k.contents) || gate::declined(p, &c, on, &k.contents) {
            return suite(p, c, k, ctx);
        }
        k.queue = false;
        p.journal("merge.reverify", Some(c.id.as_str()), serde_json::json!({}));
        let text = "Merging the base in changed files its review rounds approved: its verification started again outside the merge queue, its round and its suite. Once both pass, it enters the queue again at its back, or its approved PR merges while your approval stands.";
        say(p, &c.id, "verification started again outside the merge queue", text);
    }
    let Some(commit) = k.commit.clone() else { return };
    // A PR entry's new head that keeps its failed round asks no round; its suite still runs (fac-350).
    let kept = gate::keeps(p, &c, k.review_changes.as_ref());
    let passed = gate::settles(p, &c, &k.contents) || gate::declined(p, &c, on, &k.contents);
    let owed = !kept && gate::reviewed(&c) && !passed;
    if let Some(rid) = gate::blocked(p, &c).filter(|_| owed) {
        return held(p, c, k, on, &rid);
    }
    let id = c.id.clone();
    c.landing = Some(Landing::Test(k.clone()));
    p.put_card(c.clone());
    // The round is asked first, so a suite waiting for a slot never holds it back.
    if owed {
        gate::ask(p, &c, &commit, on, &k.contents, ctx);
    }
    if let Some(c) = p.card(&id) {
        suite(p, c, k, ctx);
    }
}

// Andrei's review of its branch holds its gate's id: it holds its place, and `/resume` once that
// review is done runs its checks again, then its round and its suite.
fn held(p: &mut Poster<'_>, mut c: Card, k: Candidate, on: &Sha, rid: &CardId) {
    let Some(commit) = k.commit.clone() else { return };
    let (card, clone) = (c.id.clone(), k.clone);
    let purpose = Purpose::TrainRun { card, commit, on: on.clone(), clone, checks: true, test: None, unwalled: false };
    c.landing = Some(Landing::Test(Candidate { checked: false, ..k }));
    let why = format!("{rid}, Andrei's review of its branch, is under way: its review card waits for `done {rid}`");
    hold(p, c, purpose, AskKind::Error, &why);
}

/// Its review round ended, none under way: its verification completes once its suite ended too.
pub fn reviewed(p: &mut Poster<'_>, id: &CardId, ctx: Ctx<'_>) {
    let Some(c) = p.card(id) else { return };
    let Some(Landing::Review(k)) = c.landing.clone() else { return };
    if round_open(p, &c) {
        return;
    }
    let project = c.project.clone();
    complete(p, c, k, ctx);
    maintain(p, project.as_str(), ctx);
}

/// Whether a card's review round is under way, or a failed one's proofs are still being copied: its verification
/// waits for it (fac-377).
pub fn round_open(p: &Poster<'_>, c: &Card) -> bool {
    gate::card_for(p, c).and_then(|rc| rc.review).is_some_and(|r| r.open().is_some() || r.proofs_pending())
}

/// A failed round of our card under verification: held until its suite ends too, never cutting it short (fac-375, fac-680).
pub fn hold_round(p: &mut Poster<'_>, c: &Card, held: HeldRound, ctx: Ctx<'_>) -> bool {
    let (mut c, id) = (c.clone(), c.id.clone());
    let k = match c.landing.as_mut() {
        Some(Landing::Test(k)) if k.checked => k,
        Some(Landing::Review(k)) => k,
        _ => return false,
    };
    k.round = Some(held);
    p.put_card(c);
    reviewed(p, &id, ctx);
    true
}

/// The whole suite, unless a pass is remembered for its tree and the suite's command (FAC-9's
/// passes, 4.9.6).
pub(crate) fn suite(p: &mut Poster<'_>, c: Card, k: Candidate, ctx: Ctx<'_>) {
    let Some(tree) = k.tree.clone() else { return };
    let command = suite_command(&c, ctx);
    let remembered = p.state.passes.iter().any(|x| x.project == c.project && x.tree == tree && x.command == command);
    let Some(test) = command.first().cloned().filter(|_| !remembered) else {
        let text = format!("The suite passed on this tree before ({tree}): it isn't run again.");
        say(p, &c.id, "the suite's pass remembered", &text);
        return tested(p, c, k, None, ctx);
    };
    let on = k.on.clone();
    start(p, c, k, on, false, Some(test), ctx);
}

// A legacy success leaves wall-test records unproven, so it owes an explicit walled run.
// The factory's cache entry must prove both phases on the same candidate (fac-65).
pub(crate) fn suite_passed(p: &mut Poster<'_>, c: Card, k: Candidate, purpose: &Purpose, run: &FinalRun, ctx: Ctx<'_>) {
    let Purpose::TrainRun { card, commit, on, clone, test: Some(test), unwalled, .. } = purpose else {
        return tested(p, c, k, None, ctx);
    };
    if !run.ran {
        return tested(p, c, k, None, ctx);
    }
    if walls_owed(&c, ctx) && (!unwalled || run.phase.as_deref() != Some("unwalled")) {
        // A recovered legacy run made no wall-test records: only an explicit walled pass owes host work.
        let unwalled = !unwalled && run.phase.as_deref() == Some("walled");
        let (card, commit, on, clone, test) = (card.clone(), commit.clone(), on.clone(), *clone, Some(test.clone()));
        return job(p, c, k, Purpose::TrainRun { card, commit, on, clone, checks: false, test, unwalled }, ctx);
    }
    // Remembered under the command its job ran, whatever the config says now (M4.11).
    let mut command = vec![test.clone()];
    if *unwalled {
        command.push(WALLS.to_owned());
    }
    if let Some(tree) = k.tree.clone() {
        remember(p, &c, &tree, command);
    }
    tested(p, c, k, None, ctx);
}

/// The candidate's run: the checkout unchanged around it, its checks and its suite passed; a failure is kept for
/// its verification's end; what can't be trusted holds the card, with a red ask (decision 14).
pub fn ran(p: &mut Poster<'_>, c: Card, purpose: &Purpose, run: FinalRun, ctx: Ctx<'_>) {
    if stale(p, &c, purpose) {
        return;
    }
    let Some(Landing::Test(mut k)) = c.landing.clone() else { return };
    let (Some(commit), Some(tree)) = (k.commit.clone(), k.tree.clone()) else { return };
    // Checks that can't be trusted first: with their folder dirty nothing ran at all (4.7.2).
    let checks = run.checks.as_ref().map(crate::lifecycle::gates::verdict);
    if let Some(Err(why)) = &checks {
        let words = format!("the checks on {} can't be trusted: {why}", commit.short());
        return hold(p, c, purpose.clone(), AskKind::Checks, &words);
    }
    if let Err(why) = unchanged(&run, &commit, &tree) {
        return hold(p, c, purpose.clone(), AskKind::Error, &format!("its run's result is discarded: {why}"));
    }
    let mut failures = Vec::new();
    if let (Some(Ok(n)), Some(k)) = (&checks, run.checks.as_ref())
        && *n > 0
    {
        let ours = c.pr.and_then(|n| super::approval::owner(p.state, c.project.as_str(), n));
        let colleague = crate::reviews::gate::entry(&c) && ours.is_none();
        if let Some(report) = &k.report {
            failures.push(crate::reviews::checks::output(report, *n, colleague));
        }
    }
    if let Some(words) = infra(c.project.as_str(), &run, ctx) {
        return hold(p, c, purpose.clone(), AskKind::Error, &words);
    }
    let mut c = c;
    if let (true, Some(code)) = (run.ran, run.exit.filter(|e| *e != 0)) {
        let unwalled = matches!(purpose, Purpose::TrainRun { unwalled: true, .. });
        let suite = if unwalled { "The suite's wall tests, outside the wall," } else { "The suite" };
        // Its log's copies, for each delivery of its news to name.
        c.news = (!run.logs.is_empty()).then(|| SuiteNews { logs: run.logs.clone() });
        failures.push(format!("{suite} failed (exit {code}):\n{}", run.output));
        k.failing = run.failing.clone().unwrap_or_default();
    }
    let project = c.project.clone();
    let what = if k.queue { "The merge queue's run of its candidate" } else { "The run of its candidate" };
    let failure = Some(format!("{what} {commit} (its head merged onto {}) failed.\n\n{}", k.on, failures.join("\n\n")));
    match (failures.is_empty(), purpose) {
        // Its checks passed: its review round and its suite are next, side by side (fac-375).
        (true, Purpose::TrainRun { on, test: None, .. }) if !k.checked => {
            checked(p, c, k, on, ctx);
        }
        // An entry's checks found something: its round weighs what they say, or Andrei's approval (fac-356).
        (false, Purpose::TrainRun { on, test: None, .. }) if !k.checked && crate::reviews::gate::entry(&c) => {
            c.checks_output = Some(failures.join("\n\n"));
            checked(p, c, k, on, ctx);
        }
        (true, _) => suite_passed(p, c, k, purpose, &run, ctx),
        (false, _) => tested(p, c, k, failure, ctx),
    }
    maintain(p, project.as_str(), ctx);
}

// Its suite ended, or none was owed: its verification completes once no round is under way (fac-375).
fn tested(p: &mut Poster<'_>, mut c: Card, mut k: Candidate, failure: Option<String>, ctx: Ctx<'_>) {
    k.failure = failure;
    if round_open(p, &c) {
        c.landing = Some(Landing::Review(k));
        return p.put_card(c);
    }
    complete(p, c, k, ctx);
}

// Both its round and its suite ended: an entry reports; our card, all passed, is verified; anything failed goes to
// its foreman at once, all of it in one request. A round asked since decides first.
fn complete(p: &mut Poster<'_>, mut c: Card, k: Candidate, ctx: Ctx<'_>) {
    if gate::entry(&c) {
        let failure = k.failure.clone();
        return super::entries::finish(p, c, k, failure, ctx);
    }
    let at = k.commit.as_ref().map_or("?", |x| x.short()).to_owned();
    let why = |has_round: bool, has_fail: bool| match (has_round, has_fail) {
        (true, true) => format!("its review round and its candidate's run on {at} failed"),
        (true, false) => format!("its review round on {at} failed"),
        _ => format!("its candidate's run on {at} failed"),
    };
    match k.round {
        Some(HeldRound::Triage(mut held)) => {
            let reason = why(true, k.failure.is_some());
            held.suite_failure = k.failure;
            held.from_verification = true;
            held.tip = Some(k.on).or_else(|| super::order::base_on(p, c.project.as_str()));
            crate::reviews::triage::leave_for_triage(p, c, held, &reason, ctx);
        }
        None if k.failure.is_none() => {
            let declined = gate::declined(p, &c, &k.on, &k.contents);
            if k.queue || !gate::reviewed(&c) || gate::approves(p, &c, &k.contents) || declined {
                return super::train::verified(p, c, k, ctx);
            }
            c.landing = Some(Landing::Review(k));
            p.put_card(c);
        }
        other => {
            let id = c.id.clone();
            let round_text = match other {
                Some(HeldRound::Text { text }) => Some(text),
                Some(HeldRound::Triage(_)) | None => None,
            };
            let reason = why(round_text.is_some(), k.failure.is_some());
            let text = round_text.iter().chain(&k.failure).cloned().collect::<Vec<_>>().join("\n\n");
            super::members::leave(p, c, &reason, &text, ctx);
            super::suite_news::told(p, &id);
        }
    }
}

// FAC-9's pass, by project, tree and the suite's command it ran (4.9.6, M4.11).
fn remember(p: &mut Poster<'_>, c: &Card, tree: &str, command: Vec<String>) {
    let known = p.state.passes.iter().any(|x| x.project == c.project && x.tree == tree && x.command == command)
        || p.other.iter().any(
            |ch| matches!(ch, Change::AddPass(x) if x.project == c.project && x.tree == tree && x.command == command),
        );
    if !known {
        p.other.push(Change::AddPass(Pass { project: c.project.clone(), tree: tree.to_owned(), command }));
    }
}

// The suite's last lines, joined, for a red ask.
fn tail(output: &str) -> String {
    let last: Vec<&str> = output.lines().rev().take(5).collect();
    last.into_iter().rev().collect::<Vec<_>>().join(" / ")
}

// Why a run's suite couldn't run, when it couldn't: a runtime missing, a signal, or an exit the
// project calls its infrastructure's, which is no defect of the code (decision 14).
fn infra(project: &str, run: &FinalRun, ctx: Ctx<'_>) -> Option<String> {
    let listed = ctx.config.projects.get(project).map(|x| x.infra_exits.as_slice()).unwrap_or_default();
    match (run.ran, run.exit) {
        (false, _) | (true, Some(0)) => None,
        (true, Some(126 | 127) | None) => {
            let exit = run.exit.map_or("a signal ended it".to_owned(), |e| format!("exit {e}"));
            Some(format!("the suite couldn't run ({exit}): {}", tail(&run.output)))
        }
        (true, Some(code)) if u8::try_from(code).is_ok_and(|e| listed.contains(&e)) => {
            Some(format!("the suite's infrastructure failed (exit {code}): {}", tail(&run.output)))
        }
        (true, Some(_)) => None,
    }
}

// The commit and the tracked files were the candidate's before the run, and still are after.
fn unchanged(run: &FinalRun, head: &Sha, tree: &str) -> Result<(), String> {
    let (b, a) = (&run.before, &run.after);
    let at = |h: &Option<Sha>| h.as_ref().map_or("no commit".to_owned(), |x| x.short().to_owned());
    if b.head.as_ref() != Some(head) {
        return Err(format!("its clone was at {} as it started, not at {}", at(&b.head), head.short()));
    }
    if !b.clean || b.tree.as_deref() != Some(tree) {
        return Err("its clone had changes not committed as it started".to_owned());
    }
    if a.head != b.head {
        return Err(format!("its clone's commit moved to {} while it ran", at(&a.head)));
    }
    if !a.tracked_clean || a.tree != b.tree {
        return Err("tracked files of its clone changed while it ran".to_owned());
    }
    Ok(())
}

// What a pass of the factory's own suite is remembered under beside its command: its wall tests
// passed outside the wall too. A pass remembered without it proves none of them (fac-65).
const WALLS: &str = "its wall tests, outside every wall (fac-65)";

// The project's suite command, as passes are remembered under (4.9.6); the factory's own, with
// `WALLS`.
fn suite_command(c: &Card, ctx: Ctx<'_>) -> Vec<String> {
    let Some(p) = ctx.config.projects.get(&c.project) else { return Vec::new() };
    let mut command = vec![p.test.clone()];
    if walls_owed(c, ctx) {
        command.push(WALLS.to_owned());
    }
    command
}

// Whether the card's suite has wall tests that need a run outside every wall: the factory's own
// project's, whose walled jobs can start no wall inside them (fac-65).
fn walls_owed(c: &Card, ctx: Ctx<'_>) -> bool {
    c.project == ctx.config.factory.project
}
