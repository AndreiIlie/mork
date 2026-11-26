//! Engine-native failed-round findings triage (fac-680, fac-689, fac-694): holds a non-delegated
//! card's failed round or a delegated review stalemate behind a triage ask, else sends findings directly.

use std::collections::{BTreeMap, BTreeSet};

use mork_types::ask::{AskKind, Color};
use mork_types::card::{Card, Landing, Stage};
use mork_types::landing::HeldRound;
use mork_types::reply::Role;
use mork_types::review::{PendingTriage, Raised, Round, TriageItem, Verdict};
use mork_types::state::Sender;
use mork_types::suites::Told;
use mork_types::{AskId, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::messages::asks::{self, Raise};
use crate::post::Poster;
use crate::state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Fix,
    Decline,
    Foreman,
}

/// Holds a failed review round with findings for triage when delegation is off or on a delegated
/// stalemate, else sends them straight to the foreman as foreman-decides (fac-680, fac-689, fac-694).
pub(super) fn from_verdict(p: &mut Poster<'_>, c: Card, rc: &Card, round: &Round, ctx: Ctx<'_>) {
    if !c.stage.is_live() || c.landing == Some(Landing::HandMerged) {
        return;
    }
    let items: Vec<TriageItem> =
        round.named.iter().enumerate().map(|(i, &n)| TriageItem { pos: (i + 1) as u32, n }).collect();
    let held = PendingTriage {
        ask: AskId::default(),
        review: rc.id.clone(),
        round: round.n,
        commit: round.commit.clone(),
        head: round.submitted.clone().or_else(|| round.head.clone()),
        items,
        suite_failure: None,
        from_verification: false,
        tip: None,
        stalemate: false,
    };
    if crate::queue::runs::hold_round(p, &c, HeldRound::Triage(held.clone()), ctx) {
        return;
    }
    if c.stage == Stage::Merging && c.landing.is_some() && !crate::queue::order::landing(&c) {
        let why = format!("its review round {} failed", round.n);
        let held = PendingTriage { from_verification: true, ..held };
        return leave_for_triage(p, c, held, &why, ctx);
    }
    if c.delegated {
        if let Some(st) = stalemate_prev(rc, &held) {
            return open(p, c, rc, held, Some(st));
        }
        return dispatch_direct(p, c, &held, None, ctx);
    }
    open(p, c, rc, held, None);
}

/// Leaves verification for triage when delegation is off or a delegated round stalled, else sends
/// findings straight to the foreman as foreman-decides (fac-680, fac-689, fac-694).
pub(crate) fn leave_for_triage(p: &mut Poster<'_>, mut c: Card, mut held: PendingTriage, why: &str, ctx: Ctx<'_>) {
    let project = c.project.clone();
    let tip = crate::queue::members::exit_verification(p, &mut c, why);
    if held.tip.is_none() {
        held.tip = tip;
    }
    if c.delegated {
        if let Some(rc) = p.card(&held.review)
            && let Some(st) = stalemate_prev(&rc, &held)
        {
            open(p, c, &rc, held, Some(st));
        } else {
            p.put_card(c.clone());
            dispatch_direct(p, c, &held, Some(why), ctx);
        }
    } else if let Some(rc) = p.card(&held.review) {
        open(p, c, &rc, held, None);
    } else {
        p.put_card(c);
    }
    crate::queue::train::maintain(p, project.as_str(), ctx);
}

/// Saves `c`, closing any open ungiven non-stalemate triage ask and sending its findings straight
/// to the foreman as foreman-decides when delegation turns on (fac-689, fac-694).
pub(crate) fn release_on_delegate(p: &mut Poster<'_>, mut c: Card, ctx: Ctx<'_>) {
    let pending = c
        .triage
        .as_ref()
        .filter(|t| !t.stalemate && p.get_ask(t.ask.as_str()).is_some_and(|a| a.open && !a.given))
        .map(|t| t.ask.clone())
        .and_then(|ask| {
            asks::close_one(p, ask.as_str());
            c.triage.take()
        });
    p.put_card(c.clone());
    if let Some(pending) = pending {
        dispatch_direct(p, c, &pending, None, ctx);
    }
}

// Two consecutive completed rounds under the current plan stall when none of the prior round's
// findings were resolved and every one repeats unresolved in the new round (fac-694).
fn stalemate_prev(rc: &Card, pending: &PendingTriage) -> Option<(u32, usize)> {
    let review = rc.review.as_deref()?;
    let prev = review
        .current()
        .rev()
        .find(|r| r.n < pending.round && matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)))?;
    let prev_set: BTreeSet<u32> = prev.named.iter().copied().collect();
    if prev.verdict != Some(Verdict::Failed) || prev_set.is_empty() {
        return None;
    }
    let resolved = resolved_of(rc);
    if prev_set.iter().any(|n| resolved.contains(n)) {
        return None;
    }
    let curr_open: BTreeSet<u32> = pending.items.iter().map(|i| i.n).filter(|n| !resolved.contains(n)).collect();
    (!curr_open.is_empty() && prev_set.is_subset(&curr_open)).then_some((prev.n, prev_set.len()))
}

fn stalemate_summary(prev: u32, repeated: usize, total: usize) -> String {
    let word = if repeated == 1 { "finding" } else { "findings" };
    if total > repeated {
        format!("{repeated} unresolved {word} repeated from round {prev} ({total} for triage).")
    } else {
        format!("{repeated} unresolved {word} repeated from round {prev}.")
    }
}

fn open(p: &mut Poster<'_>, mut c: Card, rc: &Card, mut pending: PendingTriage, stalemate: Option<(u32, usize)>) {
    let id = c.id.clone();
    asks::close(p, id.as_str(), &[AskKind::Triage]);
    pending.stalemate = stalemate.is_some();
    let color = if pending.stalemate { Color::Red } else { Color::Yellow };
    let text = ask_text(&c, rc, &pending, stalemate);
    let count = pending.items.len();
    let (headline, notice) = match stalemate {
        Some((prev, repeated)) => (
            "review stalemate waits for triage",
            format!("Review round {} stalled: {}", pending.round, stalemate_summary(prev, repeated, count)),
        ),
        None => {
            let word = if count == 1 { "finding waits" } else { "findings wait" };
            ("findings wait for triage", format!("Review round {} failed: {count} {word} for triage.", pending.round))
        }
    };
    let mut raise = Raise::new(id.as_str(), AskKind::Triage, color, text);
    raise.seq = Some(crate::queue::members::say(p, &id, headline, &notice));
    pending.ask = asks::raise(p, raise);
    c.triage = Some(pending);
    p.put_card(c);
}

/// Clears a pending triage, closes its ask, and drops any retained suite failure logs.
pub fn withdraw(p: &mut Poster<'_>, c: &mut Card) {
    if c.triage.take().is_some() {
        asks::close(p, c.id.as_str(), &[AskKind::Triage]);
        c.news = None;
    }
}

/// `triage <card>`: applies Decline choices, closes the ask, and either fast-passes or wakes the foreman once.
pub(super) fn answer(
    state: &State,
    from: &Sender,
    card: &CardId,
    fix: &[String],
    decline: &[String],
    foreman: &[String],
    ctx: Ctx<'_>,
) -> Decision {
    let id = CardId::new(card.as_str().to_lowercase());
    let Some(mut c) = state.cards.get(&id).cloned() else {
        return Decision::refused(format!("no card `{card}`"));
    };
    if c.paused {
        return Decision::refused(format!("{id} is paused: `resume {id}` first"));
    }
    if !c.stage.is_live() || c.landing == Some(Landing::HandMerged) {
        return Decision::refused(format!("{id} has no open findings triage"));
    }
    let Some(pending) = c.triage.clone() else {
        return Decision::refused(format!("{id} has no open findings triage"));
    };
    if !state.asks.get(&pending.ask).is_some_and(|a| a.open) {
        return Decision::refused(format!("{id} has no open findings triage"));
    }
    let choices = match parse_choices(&pending, fix, decline, foreman) {
        Ok(map) => map,
        Err(msg) => return Decision::refused(msg),
    };
    let mut p = Poster::for_ctx(state, ctx);
    let pre_resolved = apply_declines(&mut p, from, &pending, &choices, ctx);
    asks::close_one(&mut p, pending.ask.as_str());
    c.triage = None;
    p.put_card(c.clone());
    record_triage(&mut p, from, &id, &pending, &choices, &pre_resolved);
    dispatch(&mut p, c, &pending, &choices, &pre_resolved, from, ctx);
    let (f, d_list, m) = split_by_choice(&pending, &choices, &pre_resolved);
    let body =
        json!({"triage": {"card": id.as_str(), "round": pending.round, "fix": f, "decline": d_list, "foreman": m}});
    let mut decision = Decision::ok(body);
    p.finish(&mut decision);
    decision
}

fn parse_choices(
    pending: &PendingTriage,
    fix: &[String],
    decline: &[String],
    foreman: &[String],
) -> Result<BTreeMap<u32, Choice>, String> {
    let expected: BTreeSet<u32> = pending.items.iter().map(|i| i.n).collect();
    let mut out = BTreeMap::new();
    for (list, choice) in [(fix, Choice::Fix), (decline, Choice::Decline), (foreman, Choice::Foreman)] {
        for raw in list {
            let trimmed = raw.trim();
            let n: u32 =
                trimmed.parse().ok().filter(|&v| v > 0).ok_or_else(|| format!("`{raw}` is not a finding number"))?;
            if !expected.contains(&n) {
                return Err(format!("finding #{n} is not in round {}'s triage", pending.round));
            }
            if out.insert(n, choice).is_some() {
                return Err(format!("finding #{n} was named more than once"));
            }
        }
    }
    let missing: Vec<String> = expected.iter().filter(|n| !out.contains_key(n)).map(|n| format!("#{n}")).collect();
    if !missing.is_empty() {
        return Err(format!("every finding needs a choice; missing {}", missing.join(", ")));
    }
    Ok(out)
}

fn apply_declines(
    p: &mut Poster<'_>,
    from: &Sender,
    pending: &PendingTriage,
    choices: &BTreeMap<u32, Choice>,
    ctx: Ctx<'_>,
) -> BTreeSet<u32> {
    let Some(mut rc) = p.card(&pending.review) else { return BTreeSet::new() };
    let pre_resolved = resolved_of(&rc);
    let mut replies = Vec::new();
    let mut changed = false;
    for item in &pending.items {
        if choices.get(&item.n) != Some(&Choice::Decline) || pre_resolved.contains(&item.n) {
            continue;
        }
        if let Some(comment) = super::findings::mark_resolved(p, &mut rc, item.n, from, true) {
            changed = true;
            if let Some(comment) = comment {
                replies.push(comment);
            }
        }
    }
    if changed {
        p.put_card(rc.clone());
        for comment in replies {
            super::findings::reply_declined(p, &rc, comment, ctx);
        }
    }
    pre_resolved
}

fn split_by_choice(
    pending: &PendingTriage,
    choices: &BTreeMap<u32, Choice>,
    pre_resolved: &BTreeSet<u32>,
) -> (Vec<u32>, Vec<u32>, Vec<u32>) {
    let mut fix = Vec::new();
    let mut decline = Vec::new();
    let mut foreman = Vec::new();
    for item in &pending.items {
        if pre_resolved.contains(&item.n) {
            continue;
        }
        match choices.get(&item.n) {
            Some(Choice::Fix) => fix.push(item.n),
            Some(Choice::Decline) => decline.push(item.n),
            Some(Choice::Foreman) => foreman.push(item.n),
            None => {}
        }
    }
    (fix, decline, foreman)
}

fn record_triage(
    p: &mut Poster<'_>,
    from: &Sender,
    id: &CardId,
    pending: &PendingTriage,
    choices: &BTreeMap<u32, Choice>,
    pre_resolved: &BTreeSet<u32>,
) {
    let (fix, decline, foreman) = split_by_choice(pending, choices, pre_resolved);
    let who = actor(from, &pending.ask);
    let fmt = |label: &str, ns: &[u32]| {
        (!ns.is_empty()).then(|| {
            let nums: Vec<String> = ns.iter().map(|n| format!("#{n}")).collect();
            format!("{label} {}", nums.join(" "))
        })
    };
    let parts: Vec<String> =
        [fmt("fix", &fix), fmt("declined", &decline), fmt("foreman decides", &foreman)].into_iter().flatten().collect();
    let line = format!("{who} triaged round {}: {}", pending.round, parts.join(" · "));
    crate::queue::members::say(p, id, &format!("triaged round {}", pending.round), &line);
    let data = json!({
        "round": pending.round,
        "review": pending.review,
        "by": from.label(),
        "fix": fix,
        "decline": decline,
        "foreman": foreman,
    });
    p.journal("review.triaged", Some(id.as_str()), data);
}

fn resolved_of(rc: &Card) -> BTreeSet<u32> {
    rc.review.as_ref().map(|r| r.raised.iter().filter(|x| x.resolved).map(|x| x.n).collect()).unwrap_or_default()
}

fn dispatch_direct(p: &mut Poster<'_>, c: Card, pending: &PendingTriage, why: Option<&str>, ctx: Ctx<'_>) {
    let pre_resolved = p.card(&pending.review).map(|rc| resolved_of(&rc)).unwrap_or_default();
    let chosen: Vec<(&TriageItem, Choice)> =
        pending.items.iter().filter(|i| !pre_resolved.contains(&i.n)).map(|i| (i, Choice::Foreman)).collect();
    if (!chosen.is_empty() || pending.suite_failure.is_some())
        && let Some(why) = why
    {
        let id = c.id.clone();
        crate::queue::members::say(p, &id, "returned to its foreman", &format!("{id} returned to its foreman: {why}."));
    }
    send_chosen(p, c, pending, &chosen, &[], None, ctx);
}

fn dispatch(
    p: &mut Poster<'_>,
    c: Card,
    pending: &PendingTriage,
    choices: &BTreeMap<u32, Choice>,
    pre_resolved: &BTreeSet<u32>,
    from: &Sender,
    ctx: Ctx<'_>,
) {
    let chosen: Vec<(&TriageItem, Choice)> = pending
        .items
        .iter()
        .filter(|i| !pre_resolved.contains(&i.n))
        .filter_map(|i| match choices.get(&i.n) {
            Some(ch @ (Choice::Fix | Choice::Foreman)) => Some((i, *ch)),
            _ => None,
        })
        .collect();
    let declined: Vec<String> = pending
        .items
        .iter()
        .filter(|i| !pre_resolved.contains(&i.n) && choices.get(&i.n) == Some(&Choice::Decline))
        .map(|i| format!("#{}", i.n))
        .collect();
    send_chosen(p, c, pending, &chosen, &declined, Some(from), ctx);
}

fn send_chosen(
    p: &mut Poster<'_>,
    mut c: Card,
    pending: &PendingTriage,
    chosen: &[(&TriageItem, Choice)],
    declined: &[String],
    by: Option<&Sender>,
    ctx: Ctx<'_>,
) {
    let id = c.id.clone();
    if chosen.is_empty() && pending.suite_failure.is_none() {
        crate::queue::suite_news::told(p, &id);
        if c.paused {
            c.gates.checks_waiting = c.head.clone();
            p.put_card(c);
        } else if crate::lifecycle::gates::checks_stand(p, &c, ctx) {
            crate::lifecycle::gates::passed(p, c, ctx);
        } else {
            crate::lifecycle::gates::start_checks(p, c, ctx);
        }
        return;
    }
    let text = request_text(p, &c, pending, chosen, declined, by);
    if pending.from_verification {
        crate::queue::members::send_return_request(p, &c, pending.tip.clone(), &text, ctx);
    } else {
        let told = Told::Agent {
            card: id.clone(),
            role: Role::Foreman,
            headline: format!("Review round {} failed", pending.round),
            text,
        };
        crate::queue::suite_news::send(p, &id, told);
    }
    crate::queue::suite_news::told(p, &id);
}

fn request_text(
    p: &Poster<'_>,
    c: &Card,
    pending: &PendingTriage,
    chosen: &[(&TriageItem, Choice)],
    declined: &[String],
    by: Option<&Sender>,
) -> String {
    let mut parts = Vec::new();
    if !chosen.is_empty() {
        parts.push(findings_block(p, c, pending, chosen, declined, by));
    }
    if let Some(suite) = &pending.suite_failure {
        parts.push(suite.clone());
    }
    parts.join("\n\n")
}

fn findings_block(
    p: &Poster<'_>,
    c: &Card,
    pending: &PendingTriage,
    chosen: &[(&TriageItem, Choice)],
    declined: &[String],
    by: Option<&Sender>,
) -> String {
    let rc = p.card(&pending.review);
    let moved = super::verdict::moved_on(pending.head.as_ref().or(Some(&pending.commit)), c);
    let triaged = by.map_or(String::new(), |from| format!(" Triaged by {}.", actor(from, &pending.ask)));
    let mut head = format!(
        "Review round {} of {} failed, on commit {}.{moved}{triaged} \
         Findings to address below ({}), each by its place in this round (`<i>-` for its proofs) and its number on {} (`mork ctl resolve {} <n>`).",
        pending.round,
        c.id,
        pending.commit.short(),
        chosen.len(),
        pending.review,
        pending.review
    );
    if !declined.is_empty() {
        head.push_str(&format!(" Declined and already closed: {}.", declined.join(", ")));
    }
    let listed: Vec<String> = chosen
        .iter()
        .filter_map(|(item, ch)| {
            let f = rc.as_ref().and_then(|r| finding_of(r, item.n))?;
            let tag = if *ch == Choice::Foreman { "yours to decide" } else { "fix" };
            Some(format!(
                "{}. (#{}) [{tag}] {}:{}: {}\nProof: {}",
                item.pos, item.n, f.file, f.line, f.summary, f.proof
            ))
        })
        .collect();
    let proofs = rc
        .as_ref()
        .and_then(|r| r.review.as_ref()?.rounds.iter().find(|x| x.n == pending.round))
        .map(super::verdict::proofs)
        .unwrap_or_default();
    format!("{head}\n\n{}{proofs}", listed.join("\n\n"))
}

fn actor(from: &Sender, ask: &AskId) -> String {
    match from {
        Sender::Operator => "the operator".to_owned(),
        Sender::Orchestrator(_) => format!("{}, on the operator's delegation, answering {ask}", from.label()),
        Sender::Agent(_) | Sender::Engine | Sender::Pa => from.label(),
    }
}

fn ask_text(c: &Card, rc: &Card, pending: &PendingTriage, stalemate: Option<(u32, usize)>) -> String {
    let n = pending.items.len();
    let mut text = match stalemate {
        Some((prev, repeated)) => {
            format!("Review round {} of {} stalled: {}", pending.round, c.id, stalemate_summary(prev, repeated, n))
        }
        None => {
            let word = if n == 1 { "finding waits" } else { "findings wait" };
            format!("Review round {} of {} failed: {n} {word} for triage.", pending.round, c.id)
        }
    };
    for item in &pending.items {
        if let Some(f) = finding_of(rc, item.n) {
            let loc = if f.line > 0 { format!("{}:{}", f.file, f.line) } else { f.file.clone() };
            text.push_str(&format!("\n#{} {loc} {}", item.n, f.summary));
        }
    }
    text
}

fn finding_of(rc: &Card, n: u32) -> Option<&Raised> {
    rc.review.as_ref()?.raised.iter().find(|f| f.n == n)
}
