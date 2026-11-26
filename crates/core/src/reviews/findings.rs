//! A review's findings on its PR (fac-111): each comment kept with its finding, resolved once its raiser
//! verified its fix (`resolve`), or a decision of the operator's rules it out, said on its thread (fac-349).

use mork_types::action::{ActionStatus, ActionWhat};
use mork_types::ask::AskKind;
use mork_types::card::{Card, Mode, Stage};
use mork_types::protocol::{ErrorBody, ErrorCode};
use mork_types::reply::Finding;
use mork_types::review::{Raised, Review, Round, Verdict};
use mork_types::state::{AgentStatus, MessageKind, Sender};
use mork_types::{ActionId, CardId};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

// A finding's comment, asked once each time it is raised and kept with it: asked again, it is the
// action it has; raised again once resolved, a comment of its own, whose thread is its own.
fn ask(p: &mut Poster<'_>, id: &CardId, f: &Finding, what: ActionWhat) {
    let raised = p.card(id).and_then(|c| c.review).map(|r| r.raised).unwrap_or_default();
    let alike = || raised.iter().filter(|x| same(x, f));
    // One resolved before its comment was asked keeps it, for its thread's resolve (fac-349); raised again, the new one.
    let found = alike().find(|x| !x.resolved).or_else(|| alike().find(|x| x.resolved && x.action.is_none()));
    let Some(n) = found.map(|x| x.n) else { return };
    let mine = |c: &Card| c.review.as_ref().and_then(|r| r.raised.iter().find(|x| x.n == n)?.action.clone());
    let asked = p.card(id).as_ref().and_then(mine).and_then(|a| p.action(a.as_str()));
    if asked.is_some_and(|a| a.status != ActionStatus::Dropped) {
        return;
    }
    let Some(action) = comment(p, what, true) else { return };
    let Some(mut c) = p.card(id) else { return };
    if let Some(x) = c.review.as_deref_mut().and_then(|r| r.raised.iter_mut().find(|x| x.n == n)) {
        x.action = Some(action);
    }
    p.put_card(c);
}

/// Posts a failed run's new findings and repeats earlier open findings on their existing threads.
/// The returned notice counts new comments separately from findings carried from earlier rounds.
pub(super) fn failed(p: &mut Poster<'_>, card: &Card, ctx: Ctx<'_>) -> String {
    post_findings(p, card, ctx);
    let Some(pr) = card.pr else { return String::new() };
    let review = card.review.as_deref();
    let round = run_round(card);
    let earlier: Vec<&Raised> =
        review.into_iter().flat_map(Review::open_findings).filter(|x| x.round < round).collect();
    let new = card.findings.iter().filter(|f| !earlier.iter().any(|x| same(x, f))).count();
    let kind = if earlier.is_empty() { "" } else { "new " };
    let mut notice = format!("its {new} {kind}finding(s) are posted as PR {pr}'s comments");
    let (mut replies, mut comments) = (Vec::new(), Vec::new());
    for x in earlier {
        let (Some(thread), Some(commit)) = (x.comment, card.head.clone()) else {
            ask_earlier(p, card, x, ctx);
            comments.push(format!("#{}", x.n));
            continue;
        };
        if super::threads::silent(x) || super::threads::answered_here(p, x, card.head.as_ref()) {
            continue;
        }
        let body = format!("Still open at {}.", commit.short());
        let repo = crate::queue::pr::repo_of(card, ctx.config);
        let what = ActionWhat::ReplyComment { card: card.id.clone(), pr, comment: thread, commit, body, repo };
        comment(p, what, false);
        replies.push(format!("#{}", x.n));
    }
    if !replies.is_empty() {
        notice.push_str(&format!(
            "; {} earlier finding(s), {}, are still open and said again on their threads",
            replies.len(),
            replies.join(", ")
        ));
    }
    if !comments.is_empty() {
        notice.push_str(&format!(
            "; {} earlier finding(s), {}, are still open and their comments are requested",
            comments.len(),
            comments.join(", ")
        ));
    }
    notice
}

/// The round a run's findings come from: those of rounds before it are earlier.
pub(super) fn run_round(card: &Card) -> u32 {
    let review = card.review.as_deref();
    // The round this run's verdict came from: a later one may already be asked for a new head. A head
    // no round read kept the last failed round, whose open findings are then all earlier (fac-350).
    let last = review.and_then(|r| r.rounds.iter().rev().find(|r| r.verdict.is_some()));
    let read = |r: &Round| r.head.as_ref().zip(card.head.as_ref()).is_none_or(|(a, b)| a == b);
    last.map_or(0, |r| if read(r) { r.n } else { r.n + 1 })
}

/// An open finding as a round's texts list it, with whoever raised it.
pub(super) fn open_line(f: &Raised) -> String {
    format!("\n- #{} ({}, round {}) {}: {}", f.n, f.by, f.round, at(&f.file, f.line), f.summary)
}

/// Where a finding is: its file, at its line when it has one.
pub(super) fn at(file: &str, line: i64) -> String {
    if line > 0 { format!("{file}:{line}") } else { file.to_owned() }
}

fn same(x: &Raised, f: &Finding) -> bool {
    x.file == f.file && x.line == f.line && x.summary == f.summary
}

fn ask_earlier(p: &mut Poster<'_>, card: &Card, x: &Raised, ctx: Ctx<'_>) {
    let Some(pr) = card.pr else { return };
    let f = Finding { file: x.file.clone(), line: x.line, summary: x.summary.clone(), proof: x.proof.clone() };
    let saved = x.action.as_ref().and_then(|id| p.action(id.as_str())).map(|a| a.what);
    let what = saved.filter(|w| matches!(w, ActionWhat::Comment { .. })).unwrap_or_else(|| ActionWhat::Comment {
        card: card.id.clone(),
        pr,
        file: x.file.clone(),
        line: u64::try_from(x.line).unwrap_or(0),
        body: body(&f),
        repo: crate::queue::pr::repo_of(card, ctx.config),
    });
    ask(p, &card.id, &f, what);
}

// A finding's comment posted: Bitbucket's id kept with it, its thread resolved at once when its fix
// was verified or it was declined meanwhile.
fn kept(p: &mut Poster<'_>, id: &CardId, action: &ActionId, v: &Value, ctx: Ctx<'_>) {
    let Some(n) = v.get("id").and_then(Value::as_u64) else { return };
    let Some(mut c) = p.card(id) else { return };
    let Some(r) = c.review.as_deref_mut() else { return };
    let Some(x) = r.raised.iter_mut().find(|x| x.action.as_ref() == Some(action)) else { return };
    x.comment = Some(n);
    let (resolved, declined) = (x.resolved, x.declined);
    p.put_card(c.clone());
    if declined {
        reply_declined(p, &c, n, ctx);
    } else if resolved {
        thread(p, &c, n, ctx);
    }
}

// Said on its thread, which is resolved once it is done (`replied`): a reply of its own, whatever
// reply that head already had.
pub(super) fn reply_declined(p: &mut Poster<'_>, c: &Card, comment: u64, ctx: Ctx<'_>) {
    let (Some(pr), Some(commit)) = (c.pr, c.head.clone()) else { return };
    let body = DECLINED.to_owned();
    let repo = crate::queue::pr::repo_of(c, ctx.config);
    p.act_as(ActionWhat::ReplyComment { card: c.id.clone(), pr, comment, commit, body, repo }, true);
}

// Neutral wording naming nobody (fac-725).
const DECLINED: &str = "This finding was declined.";

// Its comment's thread resolved on its PR, the review bot's as its comment was.
pub(super) fn thread(p: &mut Poster<'_>, c: &Card, comment: u64, ctx: Ctx<'_>) {
    let Some(pr) = c.pr else { return };
    let repo = crate::queue::pr::repo_of(c, ctx.config);
    p.act(ActionWhat::ResolveComment { card: c.id.clone(), pr, comment, repo });
}

/// Marks finding `n` resolved (and `declined` when true) on `c`, journaling `review.resolved` and returning its PR comment id.
pub(super) fn mark_resolved(
    p: &mut Poster<'_>,
    c: &mut Card,
    n: u32,
    from: &Sender,
    declined: bool,
) -> Option<Option<u64>> {
    let x = c.review.as_deref_mut()?.raised.iter_mut().find(|x| x.n == n && !x.resolved)?;
    x.resolved = true;
    x.declined = declined;
    let (comment, by) = (x.comment, x.by.clone());
    let data = json!({"n": n, "by": by, "from": from.label(), "declined": declined});
    p.journal("review.resolved", Some(c.id.as_str()), data);
    Some(comment)
}

/// Resolves finding `n` and its PR thread after its raiser verifies it in the current round, or
/// `declined`, its thread first told so. After a passed round, reviewers with no remaining findings are released.
pub(super) fn resolve(state: &State, from: &Sender, card: &CardId, n: u64, declined: bool, ctx: Ctx<'_>) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(mut c) = state.cards.get(&id).cloned().filter(|c| c.mode == Mode::Review && c.review.is_some()) else {
        return Decision::refused(format!("`resolve` resolves a review card's finding; {id} isn't one"));
    };
    let any_time = declined && reviewed_foreman(from, &c);
    if let Err(words) = current(state, from, &c).or_else(|e| if any_time { Ok(()) } else { Err(e) }) {
        return Decision::refused(words);
    }
    let all = c.review.as_ref().map_or(0, |r| r.raised.len());
    let Some(found) = c.review.as_ref().and_then(|r| r.raised.iter().find(|x| u64::from(x.n) == n)) else {
        return Decision::refused(format!("{id} has no finding #{n}: it has {all}, from #1"));
    };
    if found.resolved {
        return Decision::ok(json!({"resolve": {"card": id.as_str(), "n": n, "already": true}}));
    }
    let found_n = found.n;
    let mut p = Poster::for_ctx(state, ctx);
    let Some(comment) = mark_resolved(&mut p, &mut c, found_n, from, declined) else {
        return Decision::ok(json!({"resolve": {"card": id.as_str(), "n": n, "already": true}}));
    };
    p.put_card(c.clone());
    match comment {
        Some(comment) if declined => reply_declined(&mut p, &c, comment, ctx),
        Some(comment) => thread(&mut p, &c, comment, ctx),
        None => {}
    }
    // A round that failed keeps its reviewers for the fix round; only a pass ends their work.
    let passed = c.review.as_ref().and_then(|r| r.rounds.last()).is_some_and(|r| r.verdict == Some(Verdict::Passed));
    if passed {
        super::end::release(&mut p, &id, ctx.config);
    }
    let mut d = Decision::ok(json!({"resolve": {"card": id.as_str(), "n": n, "comment": comment.is_some()}}));
    p.finish(&mut d);
    d
}

// An agent's verification counts in a turn the round under way asked about its commit, given its
// request, as its verdict does (fac-111); the operator's command stands at any time.
pub(super) fn current(state: &State, from: &Sender, rc: &Card) -> Result<(), String> {
    let Sender::Agent(key) = from else { return Ok(()) };
    let Some(open) = rc.review.as_deref().and_then(Review::open).filter(|o| o.asked.is_some()) else {
        return Err(format!("No round of {} is under way: a fix is verified in the round that asks it.", rc.id));
    };
    let agent = state.agents.get(key);
    let job = agent.and_then(|a| if let AgentStatus::Working { job } = &a.status { state.jobs.get(job) } else { None });
    let seen = job.and_then(|j| j.seen);
    let given = open.asked.is_some_and(|asked| seen.is_some_and(|s| s > asked));
    if job.is_some_and(|j| given && j.head.as_ref() == Some(&open.commit)) {
        return Ok(());
    }
    Err(format!(
        "That verification counts for nothing: it isn't from a turn of round {}, on commit {}. Verify the fix in a turn that round asks.",
        open.n,
        open.commit.short()
    ))
}

// The foreman of our card a linked review card reviews declines its findings on the operator's word at any time (fac-638).
fn reviewed_foreman(from: &Sender, rc: &Card) -> bool {
    let of = rc.review.as_ref().and_then(|r| r.of.as_ref());
    matches!(from, Sender::Agent(key) if of.is_some_and(|of| key.as_str() == format!("{of}/foreman")))
}

/// Whether `resolve` is buzzed: only the raiser of a finding verifies its fix and resolves it, the
/// review's foreman its own; the operator may resolve any, and the reviewed card's foreman decline any.
pub fn buzz(state: &State, from: &Sender, card: &CardId, n: u64, declines: bool) -> Option<ErrorBody> {
    let id = CardId::new(card.as_str().to_lowercase());
    let rc = state.cards.get(&id)?;
    let x = rc.review.as_ref()?.raised.iter().find(|x| u64::from(x.n) == n)?;
    if declines && reviewed_foreman(from, rc) {
        return None;
    }
    let name = match from {
        Sender::Operator | Sender::Orchestrator(_) | Sender::Pa => return None,
        Sender::Agent(key) => key.as_str().strip_prefix(&format!("{id}/")).unwrap_or_default(),
        Sender::Engine => "",
    };
    if !name.is_empty() && name == x.by {
        return None;
    }
    let words = format!(
        "#{n} is {}'s finding: only the reviewer that raised a finding verifies its fix and resolves it; the operator may resolve any, and the reviewed card's foreman decline any with `--declined`.",
        x.by
    );
    Some(ErrorBody::new(ErrorCode::Buzz, words))
}

/// A reply posted for this run lets the review finish once its remaining comments are posted; a
/// declined finding's thread is resolved once its reply is.
pub fn replied(p: &mut Poster<'_>, card: &CardId, id: &ActionId, ctx: Ctx<'_>) {
    super::responses::replied(p, card, id, ctx);
    let reply = p.action(id.as_str()).map(|a| a.what);
    if let Some(ActionWhat::ReplyComment { comment, body, .. }) = reply
        && body == DECLINED
        && let Some(c) = p.card(card)
        && c.review.as_ref().is_some_and(|r| r.raised.iter().any(|x| x.declined && x.comment == Some(comment)))
    {
        thread(p, &c, comment, ctx);
    }
    let mine = p
        .card(card)
        .is_some_and(|c| c.stage == Stage::Review && c.review.as_ref().is_some_and(|r| r.comments.contains(id)));
    if mine {
        super::results::done_once_posted(p, card, ctx);
    }
}

// A comment's body: the issue, a blank line, its proof (fac-260); a blank one left out.
fn body(f: &Finding) -> String {
    let parts: Vec<&str> = [f.summary.trim(), f.proof.trim()].into_iter().filter(|s| !s.is_empty()).collect();
    parts.join("\n\n")
}

// Posts a PR review card's findings as its inline comments, each one action of its own.
fn post_findings(p: &mut Poster<'_>, card: &Card, ctx: Ctx<'_>) {
    let Some(pr) = card.pr else { return };
    for f in &card.findings {
        let what = ActionWhat::Comment {
            card: card.id.clone(),
            pr,
            file: f.file.clone(),
            line: u64::try_from(f.line).unwrap_or(0),
            body: body(f),
            repo: crate::queue::pr::repo_of(card, ctx.config),
        };
        ask(p, &card.id, f, what);
    }
}

/// A review's comment asked as its run's own, told apart from an earlier run's by its action, never by a clock
/// (fac-98); `anew`, a comment of its own, whatever there is alike (fac-111).
pub fn comment(p: &mut Poster<'_>, what: ActionWhat, anew: bool) -> Option<ActionId> {
    let card = what.card().cloned()?;
    let id = p.act_as(what, anew);
    if let Some(mut c) = p.card(&card)
        && let Some(r) = c.review.as_mut()
        && !r.comments.contains(&id)
    {
        r.comments.push(id.clone());
        p.put_card(c);
    }
    Some(id)
}

/// A comment posted: a finding's keeps Bitbucket's id, its thread resolved when its fix was verified or it was
/// declined meanwhile; one Bitbucket anchored elsewhere is recorded where it landed, said once.
pub fn commented(p: &mut Poster<'_>, card: &CardId, id: &ActionId, v: &Value, ctx: Ctx<'_>) {
    kept(p, card, id, v, ctx);
    let asked = v.get("asked").and_then(Value::as_str);
    let landed = v.get("landed").and_then(Value::as_str);
    if let (Some(asked), Some(landed)) = (asked, landed)
        && asked != landed
    {
        let forge = p.card(card).map_or("the forge", |c| ctx.config.forge_label(&c.project));
        let text = format!("A comment asked at {asked} landed at {landed} on {forge}; it stays there.");
        p.post(
            card.as_str(),
            Sender::Engine,
            Some("foreman"),
            MessageKind::Update,
            &format!("comment moved on {forge}"),
            &text,
        );
    }
    // A comment an earlier, superseded run sent late ends nothing of the run after it (fac-98).
    let mine = super::results::own(p, card).iter().any(|a| a.id == *id);
    if mine {
        super::results::done_once_posted(p, card, ctx);
    }
}

/// `post <card>`: posts a review card's findings that aren't posted yet.
pub(super) fn post(state: &State, card: &str, ctx: Ctx<'_>) -> Decision {
    let Some(c) = state.cards.get(&CardId::new(card)).cloned() else {
        return Decision::refused(format!("no card `{card}`"));
    };
    if c.mode != Mode::Review || c.pr.is_none() {
        return Decision::refused(format!("`post` posts a PR review card's findings; {card} isn't one"));
    }
    let mut p = Poster::for_ctx(state, ctx);
    let before = p.all_actions().filter(|a| a.status != ActionStatus::Dropped).count();
    post_findings(&mut p, &c, ctx);
    let after = p.all_actions().filter(|a| a.status != ActionStatus::Dropped).count();
    if c.stage == Stage::Review && !c.findings.is_empty() {
        crate::messages::asks::close(&mut p, card, &[AskKind::Done]);
        super::results::done_once_posted(&mut p, &c.id, ctx);
    }
    let mut d = Decision::ok(json!({"post": {"card": card, "comments": after - before}}));
    p.finish(&mut d);
    d
}
