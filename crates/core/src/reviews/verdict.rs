//! A gate round's verdict (fac-95) counts only from a turn given the round's request and asked about its commit; any
//! other is stale. A pass approves the files reviewed; a fail's findings go to the reviewed card's foreman for fixes.

use factory_types::card::{Card, Mode, Stage};
use factory_types::ids::{CardId, Sha};
use factory_types::reply::{Finding, Outcome, Reply, Role};
use factory_types::review::{Proofs, Raised, Round, Verdict};
use factory_types::state::Job;
use serde_json::json;

use crate::decide::{Ctx, Intent};
use crate::post::Poster;

/// Where a review clone's reviewers save their executable proofs, in its `.git` (fac-263).
pub const PROOFS: &str = ".git/factory/proofs/";

/// Where a round's own proofs are kept in the review clone, numbered as its verdict's findings (fac-377).
pub fn proofs_round(round: u32) -> String {
    format!("{PROOFS}round-{round}/")
}

/// What an executable proof is, told to every seat that writes or reruns one (fac-276).
pub const PROOF_RULE: &str = "An executable proof checks the correct behaviour, so it fails on the buggy commit and passes once the finding is fixed: the same file then verifies the fix, rerun in a later round. It reaches the code through its working folder, never an absolute path to a clone, which a later round replaces. Each round's proofs stay in its own `.git/factory/proofs/round-<n>/` folder through the rounds.";

/// Where a failed round's proofs are copied in the clone of the card that fixes it.
pub fn proofs_copy(round: u32) -> String {
    format!(".git/factory/review-proofs/round-{round}/")
}

/// The review card's foreman's verdict, `review-pass` or `review-fail`.
pub fn verdict(p: &mut Poster<'_>, mut rc: Card, r: &Reply, job: &Job, ctx: Ctx<'_>) {
    let open = rc.review.as_ref().and_then(|x| x.open()).cloned();
    let seen = job.seen;
    let given = |o: &Round| o.asked.is_some_and(|asked| seen.is_some_and(|s| s > asked));
    let Some(open) = open.filter(|o| given(o) && job.head.as_ref() == Some(&o.commit)) else {
        return stale(p, &rc, job);
    };
    let passed = r.outcome == Outcome::ReviewPass;
    let waiting = rc.review.as_ref().is_some_and(|x| x.open_findings().next().is_some());
    if passed && waiting && !super::gate::entry(&rc) {
        return premature(p, &rc, &open);
    }
    let (findings, named) = if passed { (Vec::new(), Vec::new()) } else { raise(&mut rc, open.n, &r.findings) };
    // A failed round's report waits for its proofs' copy to end, and so does the round asked while it ran (fac-377).
    let fixer = if passed { None } else { fixer_of(p, &rc) };
    let said = if findings.is_empty() { format!("{}\n\n{}", r.hand.headline, r.hand.message) } else { String::new() };
    let Some(review) = rc.review.as_deref_mut() else { return };
    if let Some(round) = review.open_mut() {
        round.verdict = Some(if passed { Verdict::Passed } else { Verdict::Failed });
        round.named = named;
        round.proofs = fixer.as_ref().map(|_| Proofs::Pending { said: said.clone() });
    }
    if passed {
        review.approved.extend(open.contents());
    }
    let next = if fixer.is_some() { None } else { review.next.take() };
    rc.findings = findings.clone();
    rc.gates.review_asked = None;
    let id = rc.id.clone();
    p.put_card(rc.clone());
    let data = json!({"n": open.n, "commit": open.commit, "passed": passed, "findings": findings.len()});
    p.journal("review.verdict", Some(id.as_str()), data);
    if passed {
        super::end::release(p, &id, ctx.config);
    }
    if let Some(card) = fixer {
        return p.intent(Intent::CopyProofs { review: id, card, round: open.n });
    }
    let Some(card) = super::gate::subject(p, &rc).filter(|c| c.stage.is_live()) else { return };
    if !passed || super::gate::entry(&rc) {
        return failed(p, &rc, card, &open, (next, &said), ctx);
    }
    let text = format!(
        "Its review card's round {} passed {} file(s) as commit {} has them.",
        open.n,
        open.files.len(),
        open.commit.short()
    );
    crate::queue::members::say(p, &card.id, &format!("review round {} passed", open.n), &text);
    // What was asked while it ran is reviewed now, as far as it passed none of it.
    if let Some(n) = next {
        super::gate::ask(p, &card, &n.commit, &n.base, &n.contents(), ctx);
    }
    // Its verification completes if its suite ended too (fac-375).
    crate::queue::runs::reviewed(p, &card.id, ctx);
    crate::queue::train::maintain(p, card.project.as_str(), ctx);
}

// A verdict's report, a failed round's once its proofs' copy ended or none was owed: an entry's run reports once its
// round and its suite both ended, whatever either's result (fac-375); our card's findings go to its foreman.
fn failed(p: &mut Poster<'_>, rc: &Card, card: Card, round: &Round, (next, said): (Option<Round>, &str), ctx: Ctx<'_>) {
    if super::gate::entry(rc) {
        if let Some(n) = next {
            super::gate::ask(p, &card, &n.commit, &n.base, &n.contents(), ctx);
        }
        return crate::queue::runs::reviewed(p, &rc.id, ctx);
    }
    back(p, card, rc, round, said, ctx);
}

/// The copy of a failed round's proofs ended, `Ok` with the files copied: the round records it, and its
/// failed verdict is reported, as it would have been without the wait (fac-377).
pub fn copied(p: &mut Poster<'_>, id: &CardId, round: u32, result: &Result<u32, String>, ctx: Ctx<'_>) {
    let Some(mut rc) = p.card(id) else { return };
    let Some(review) = rc.review.as_deref_mut() else { return };
    let next = review.next.take();
    let Some(r) = review.rounds.iter_mut().find(|r| r.n == round) else { return };
    let Some(Proofs::Pending { said }) = r.proofs.take() else { return };
    r.proofs = Some(match result {
        Ok(0) => Proofs::Empty,
        Ok(files) => Proofs::Copied { files: *files },
        Err(why) => Proofs::Failed { why: why.clone() },
    });
    let r = r.clone();
    p.put_card(rc.clone());
    p.journal("review.proofs", Some(id.as_str()), json!({"n": round, "result": r.proofs}));
    let Some(card) = super::gate::subject(p, &rc).filter(|c| c.stage.is_live()) else { return };
    failed(p, &rc, card, &r, (next, &said), ctx);
}

/// A failed round's proofs whose copy a restart cut short are copied again (fac-377).
pub fn recover(p: &mut Poster<'_>) {
    let cards: Vec<Card> =
        p.state.cards.values().filter(|c| c.mode == Mode::Review && c.stage.is_live()).cloned().collect();
    for rc in cards {
        let pending =
            rc.review.iter().flat_map(|r| &r.rounds).filter(|r| matches!(r.proofs, Some(Proofs::Pending { .. })));
        for round in pending.map(|r| r.n).collect::<Vec<_>>() {
            if let Some(card) = fixer_of(p, &rc) {
                p.intent(Intent::CopyProofs { review: rc.id.clone(), card, round });
            }
        }
    }
}

// A failed round's findings and their numbers, each kept with its raiser, a leading `[reviewer-2]` nothing posts
// (fac-111). One still open, named by its `#<n>`, isn't raised twice: it takes the verdict's words (fac-346).
fn raise(rc: &mut Card, round: u32, findings: &[Finding]) -> (Vec<Finding>, Vec<u32>) {
    let Some(review) = rc.review.as_deref_mut() else { return (findings.to_vec(), Vec::new()) };
    let (mut out, mut named) = (Vec::new(), Vec::new());
    for f in findings {
        let (by, rest) = raiser(&f.summary);
        let (kept, summary) = numbered(rest);
        let f = Finding { summary: summary.to_owned(), ..f.clone() };
        let same = |x: &Raised| !x.resolved && x.file == f.file && x.line == f.line && x.summary == f.summary;
        if let Some(x) = kept.and_then(|n| review.raised.iter_mut().find(|x| !x.resolved && x.n == n)) {
            x.file.clone_from(&f.file);
            x.line = f.line;
            x.summary.clone_from(&f.summary);
            x.proof.clone_from(&f.proof);
            named.push(x.n);
        } else if let Some(x) = review.raised.iter().find(|x| same(x)) {
            named.push(x.n);
        } else {
            let n = review.raised.last().map_or(1, |x| x.n + 1);
            review.raised.push(new_raised(n, round, by, &f));
            named.push(n);
        }
        out.push(f);
    }
    (out, named)
}

fn new_raised(n: u32, round: u32, by: &str, f: &Finding) -> Raised {
    Raised {
        n,
        round,
        by: by.to_owned(),
        file: f.file.clone(),
        line: f.line,
        summary: f.summary.clone(),
        proof: f.proof.clone(),
        action: None,
        comment: None,
        resolved: false,
        declined: false,
        withdrawn: false,
        thread: None,
        thread_error: None,
        answer: None,
        dropped: None,
    }
}

// A finding's number, from a leading `#<n>` naming one still open, and its summary without it.
fn numbered(summary: &str) -> (Option<u32>, &str) {
    let Some(rest) = summary.strip_prefix('#') else { return (None, summary) };
    let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    match rest[..digits].parse() {
        Ok(n) => (Some(n), rest[digits..].trim_start()),
        Err(_) => (None, summary),
    }
}

// A finding's raiser, from the leading `[reviewer-2]` its foreman tagged it with, and its summary
// without it; untagged, the foreman's own.
fn raiser(summary: &str) -> (&str, &str) {
    let tagged = summary.strip_prefix('[').and_then(|s| s.split_once(']'));
    match tagged.filter(|(n, _)| crate::agents::children::role_of(n) == Some(Role::Reviewer)) {
        Some((name, rest)) => (name, rest.trim_start()),
        None => ("foreman", summary),
    }
}

// The card whose clone fixes a failed round, and gets its proofs before a next round's clone replaces them
// (fac-263): our card's, or our PR's owner; a colleague's PR or branch has no fixer, nor has a card past its life.
fn fixer_of(p: &Poster<'_>, rc: &Card) -> Option<CardId> {
    let live = super::gate::subject(p, rc).is_some_and(|c| c.stage.is_live());
    let fixer = match rc.pr.filter(|_| super::gate::entry(rc)) {
        Some(pr) => crate::queue::approval::owner(p.state, rc.project.as_str(), pr).map(|c| c.id.clone()),
        None => super::gate::subject_of(rc).cloned(),
    };
    fixer.filter(|_| live)
}

/// The fix request's line saying what became of round `round`'s own proofs, copied into the fixer's clone.
pub fn proofs(round: &Round) -> String {
    let to = proofs_copy(round.n);
    let how = match &round.proofs {
        Some(Proofs::Copied { files }) => format!(
            "Its round's executable proofs, {files} file(s), were copied into your clone at `{to}`: a file whose name starts with `<i>-` proves finding <i> of this round, and the folder holds this round's proofs alone. Run one from your clone's root against your fix: it was written to fail on the bug and pass once fixed, and a path in it that names the review clone is for you to repoint."
        ),
        Some(Proofs::Empty) => {
            format!("Its reviewers saved no executable proofs for this round: nothing was copied to `{to}`.")
        }
        Some(Proofs::Failed { why }) => format!(
            "Copying its round's executable proofs to `{to}` failed: {why}. Whether any were saved is unknown, and the folder may be missing or partly copied."
        ),
        Some(Proofs::Pending { .. }) | None => {
            format!("No copy of its round's executable proofs to `{to}` is recorded.")
        }
    };
    format!("\n\n{how}")
}

// A verdict of a turn that wasn't given the round's request, or about another commit: said to its
// foreman, it counts for nothing.
fn stale(p: &mut Poster<'_>, rc: &Card, job: &Job) {
    let open = rc.review.as_ref().and_then(|x| x.open());
    p.journal("review.stale", Some(rc.id.as_str()), json!({"head": job.head, "open": open.map(|o| o.n)}));
    let waits = match open.filter(|o| o.asked.is_some()) {
        Some(o) => format!("Round {} waits for your verdict on commit {}.", o.n, o.commit.short()),
        None => "No round waits for a verdict now.".to_owned(),
    };
    let text = format!(
        "That verdict counts for nothing: it isn't about the round under way, or came before its request. {waits}"
    );
    crate::messages::requests::tell_agent(p, &rc.id, Role::Foreman, "verdict not counted", &text);
}

// Our card's pass while a finding waits for its raiser counts for nothing (fac-111): its round stays
// asked, for each raiser to verify and resolve its findings in a turn of it before a verdict.
fn premature(p: &mut Poster<'_>, rc: &Card, round: &Round) {
    let open: Vec<&Raised> = rc.review.iter().flat_map(|r| r.open_findings()).collect();
    let ns: Vec<u32> = open.iter().map(|f| f.n).collect();
    p.journal("review.premature", Some(rc.id.as_str()), json!({"n": round.n, "open": ns}));
    let mut text = format!(
        "That pass counts for nothing: round {} can't pass while a finding waits for its raiser to verify its fix. Each reviewer verifies its own and resolves each one fixed, `factory ctl resolve {} <n>`, or one a standing decision of Andrei's on the card rules out, with `--declined`, in a turn of this round; yours you verify and resolve yourself. Then give your verdict again, failing with what isn't fixed:",
        round.n, rc.id
    );
    for f in open {
        text.push_str(&super::findings::open_line(f));
    }
    crate::messages::requests::ask_agent(
        p,
        &rc.id,
        Role::Foreman,
        &format!("round {} waits for its findings", round.n),
        &text,
    );
}

/// The sentence saying the card has moved past the reviewed head, or none while its head is that one.
pub fn moved_on(reviewed: Option<&Sha>, card: &Card) -> String {
    match card.head.as_ref().filter(|h| Some(*h) != reviewed) {
        Some(h) => format!(
            " {}'s current head is {}, at stage {}: check which of these findings that head still has before fixing.",
            card.id,
            h.short(),
            card.stage.as_str()
        ),
        None => String::new(),
    }
}

// A failed round with findings waits for triage when delegation is off or on a delegated stalemate, and
// otherwise goes straight to the foreman when delegated; one with no findings always goes straight (fac-680, fac-689, fac-694).
fn back(p: &mut Poster<'_>, c: Card, rc: &Card, round: &Round, said: &str, ctx: Ctx<'_>) {
    if !rc.findings.is_empty() {
        return super::triage::from_verdict(p, c, rc, round, ctx);
    }
    let id = c.id.clone();
    let findings = format!("It failed with no findings: {said}");
    let proofs = proofs(round);
    let moved = moved_on(Some(round.head.as_ref().unwrap_or(&round.commit)), &c);
    let text = format!(
        "Review round {} of {id} failed, on commit {}.{moved} {findings}{proofs}",
        round.n,
        round.commit.short()
    );
    // Under verification, its suite still ends first and both results go back together (fac-375).
    if crate::queue::runs::hold_round(p, &c, factory_types::landing::HeldRound::Text { text: text.clone() }, ctx) {
        return;
    }
    if c.stage == Stage::Merging && c.landing.is_some() && !crate::queue::order::landing(&c) {
        let why = format!("its review round {} failed", round.n);
        return crate::queue::members::leave(p, c, &why, &text, ctx);
    }
    crate::messages::requests::ask_agent(p, &id, Role::Foreman, &format!("Review round {} failed", round.n), &text);
}
