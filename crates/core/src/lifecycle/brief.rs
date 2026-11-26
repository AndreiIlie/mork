//! Each recap is challenged by the card's foreman before it is offered; its approve starts the build, and ends a
//! visual card's mock with it (fac-420, fac-558, fac-636).

use mork_types::ask::{AskKind, Color};
use mork_types::card::{Approval, ApprovedVia, Card, MockStep, Mode, Stage};
use mork_types::reply::{Intent as MessageIntent, Outcome, Reply, Role};
use mork_types::state::{Agent, AgentStatus, Job, Message, Sender};
use mork_types::{AgentKey, CardId};
use serde_json::json;

use crate::decide::{Ctx, Decision, Intent};
use crate::messages::asks::{self, Raise};
use crate::messages::places::{self, Who};
use crate::post::Poster;
use crate::state::{Change, State};

/// `approve <card>` accepts an offered plan, or an offered mock.
pub(super) fn approve(state: &State, from: &Sender, id: &str, ctx: Ctx<'_>) -> Decision {
    let c = match super::card(state, id) {
        Ok(c) => c,
        Err(d) => return d,
    };
    if super::mock::open(&c) {
        return super::mock::approve(state, c, ctx);
    }
    if c.draft.is_none() || c.stage != Stage::Brief || !c.recap {
        return Decision::refused(format!("{id} has no brief to approve: it is at {}", c.stage.as_str()));
    }
    let mut p = Poster::for_ctx(state, ctx);
    let cid = c.id.clone();
    let approval = Approval { by: from.clone(), via: ApprovedVia::Approve, at_ms: ctx.now };
    approved(&mut p, c, approval, ctx);
    let mut d = Decision::ok(json!({"approve": {"card": cid.as_str()}}));
    p.finish(&mut d);
    d
}

// The approval and new foreman's news boundary commit together.
fn approved(p: &mut Poster<'_>, mut c: Card, approval: Approval, ctx: Ctx<'_>) {
    let Some(draft) = c.draft.take() else { return };
    let cid = c.id.clone();
    let record = json!({"brief": draft, "by": approval.by.label(), "via": approval.via});
    c.brief = Some(draft.clone());
    c.approval = Some(approval);
    c.recap = false;
    c.challenge = None;
    c.recaps.clear();
    c.stage = Stage::Build;
    let mocked = end_mock(&mut c);
    hand_over(p, &c, ctx.config);
    let room = places::room_of(&cid, super::facts::specifier(&c));
    p.put_card(c);
    asks::close(p, &room, &[AskKind::ApproveBrief]);
    if let Some(c) = p.card(&cid) {
        if let Some(notes) = c.rebrief.filter(|r| r.continued.is_none()).map(|r| r.notes) {
            super::reopen::continued(p, c, notes, super::reopen::Back::Brief);
        } else {
            build(p, c);
        }
    }
    crate::messages::announce::brief_approved(p, &cid, &draft);
    p.journal("brief.approved", Some(cid.as_str()), record);
    if mocked {
        crate::agents::children::release_baselines(p, &cid, true, ctx.config);
        p.journal("mock.ended", Some(cid.as_str()), json!({"step": MockStep::Approved}));
    }
}

// The plan's approve approves a visual card's brief-time mock with it (fac-636): whether one was open.
fn end_mock(c: &mut Card) -> bool {
    let Some(m) = c.mock.as_mut().filter(|m| matches!(m.step, MockStep::Open | MockStep::Baseline)) else {
        return false;
    };
    m.step = MockStep::Approved;
    m.offered = false;
    true
}

// The foreman seated at start, which challenged or, with no planner, made the plan, keeps its session and owed work
// (fac-633). Only a planner's brief turns stop, and a warm planner takes its closing turn now (fac-670).
fn hand_over(p: &mut Poster<'_>, c: &Card, config: &mork_types::config::Config) {
    if c.mode != Mode::Full {
        return;
    }
    let key = crate::messages::requests::agent_of(&c.id, super::facts::specifier(c)).key;
    if let Some(mut planner) = p.agent(&key).filter(|a| a.role == Role::Planner) {
        if matches!(planner.status, AgentStatus::Working { .. }) {
            if stop_brief_turns(p, &key) && planner.session_started {
                planner.last_turn_ms = Some(p.now);
            }
            planner.status = AgentStatus::Idle;
        }
        planner.owed = None;
        planner.wake_after = false;
        planner.completion_wake = false;
        p.put_agent(planner);
        crate::agents::closing::retire_live(p, &key, config);
        // A detached brief turn never runs `fresh::turn_ended` on settle (fac-670).
        if let Some(mut planner) =
            p.agent(&key).filter(|a| a.retiring.is_none() && a.fresh.as_ref().is_some_and(|f| f.pending))
        {
            crate::agents::fresh::turn_ended(p, &mut planner);
            p.put_agent(planner);
        }
    }
    crate::agents::children::release_researchers(p, &key, config);
}

/// A planner's queued brief messages cease to be work once its plan is approved.
pub(crate) fn planner_just_approved(p: &Poster<'_>, a: &Agent) -> bool {
    a.role == Role::Planner
        && places::card_of(p.state, &a.place).and_then(|id| p.state.cards.get(&id)).is_some_and(|c| {
            super::facts::agreeing(c)
                && p.card(&c.id).is_some_and(|next| next.stage == Stage::Build && next.approval.is_some())
        })
}

// Asks for the approved plan's build after its approval, without another recap or hand-on.
fn build(p: &mut Poster<'_>, c: Card) {
    let cid = c.id.clone();
    p.put_card(c);
    let text = format!(
        "Build {cid} to its approved plan and the decisions recorded on its card. Cut the deliverables with `deliverables` and build them yourself.\n\n\
         Submit the finished build with `built`. In the turn \
         you reply `built`, merge the base's tip, already in the clone as `origin/<base>` and kept current by the \
         engine (the clone has no remote: don't fetch), into the branch, commit, and run the checks and the tests \
         of what it touched."
    );
    crate::messages::requests::ask_build(p, &cid, &text);
}

/// The approver's name and any delegation.
pub(crate) fn approval_label(by: &Sender) -> String {
    match by {
        Sender::Operator => "the operator".to_owned(),
        Sender::Orchestrator(project) => format!("orchestrator:{project}, on the operator's delegation,"),
        by @ (Sender::Agent(_) | Sender::Engine | Sender::Pa) => by.label(),
    }
}

/// A clear, nonempty plan counts from the specification holder in its room at brief.
pub(super) fn recaps(who: &Who, r: &Reply, card: &Card) -> bool {
    r.outcome == Outcome::BriefReady
        && holds_spec(who, card)
        && !r.hand.ask
        && r.hand.color != Color::Red
        && !r.hand.message.trim().is_empty()
}

/// Whether this seat can recap its card's brief: its holder, in its own room.
pub(super) fn holds_spec(who: &Who, card: &Card) -> bool {
    card.stage == Stage::Brief
        && who.role == super::facts::holder(card)
        && who.place.as_str() == places::room_of(&card.id, who.role)
}

/// An `answer` ask its holder newly raised withdraws its waiting recap, so an approve can't take
/// the older version (fac-391); a `blocked` one doesn't.
pub fn hand_raised(p: &mut Poster<'_>, who: &Who) {
    let Some(mut card) = crate::messages::places::card_of(p.state, &who.place).and_then(|c| p.card(&c)) else {
        return;
    };
    if holds_spec(who, &card) && redraft(p, &mut card) {
        p.put_card(card);
    }
}

/// Keeps a clear plan for its foreman's challenge, or offers it on a card the challenge doesn't reach. An unread
/// correction prevents either until the planner answers it.
pub(super) fn recapped(p: &mut Poster<'_>, mut card: Card, (who, job): (&Who, &Job), r: &Reply, ctx: Ctx<'_>) {
    if outdated(p, who, &card) {
        let id = card.id.clone();
        if redraft(p, &mut card) {
            p.put_card(card);
        }
        let text = "The operator wrote to you while this recap was being made, so it isn't offered for approval. Answer him, then recap again.";
        crate::messages::requests::tell_agent(p, &id, who.role, "recap not offered: answer the operator first", text);
        return;
    }
    // Its typed sketch heads the plan every reader of words gets (fac-696).
    card.sketch = r.hand.sketch();
    let draft = match &card.sketch {
        Some(s) => format!("{}\n\n{}", s.words(), r.hand.message.trim()),
        None => r.hand.message.clone(),
    };
    card.draft = Some(draft.clone());
    card.recaps.extend(p.posted_hand(&Sender::Agent(who.key.clone())));
    card.risks = r.risks.clone();
    card.recap = false;
    let id = card.id.clone();
    // Its session wrote the recap, so it holds it as the plan (fac-623).
    if let Some(mut a) = p.agent(&who.key) {
        crate::agents::prompt::held::wrote(&mut a, job, "plan", draft.trim());
        p.put_agent(a);
    }
    p.journal("brief.planned", Some(id.as_str()), json!({}));
    if !crate::agents::required::challenge(p, &mut card, ctx.config) {
        offer(p, &mut card, ctx);
    }
    p.put_card(card);
}

/// Offers the standing plan once no challenge of it waits; mutates `c` in place for its caller to stage.
pub(crate) fn offer(p: &mut Poster<'_>, c: &mut Card, ctx: Ctx<'_>) {
    // A visual card's plan, its mock included, is the operator's alone to approve: an offered one stays as it is (fac-636).
    if c.stage != Stage::Brief || c.draft.is_none() || c.challenge.is_some() || (c.visual && c.recap) {
        return;
    }
    let holder = super::facts::holder(c);
    let words = approve_words(c, ctx.config);
    c.recap = true;
    let room = places::room_of(&c.id, holder);
    asks::close(p, &room, &[AskKind::ApproveBrief]);
    let mut raise = Raise::new(&room, AskKind::ApproveBrief, Color::Yellow, words);
    raise.agent = Some(crate::messages::requests::agent_of(&c.id, holder).key);
    asks::raise(p, raise);
}

// The approve ask's words: the recap's typed sketch (fac-696), then a visual card's mock link, which the approve
// approves with it (fac-636).
fn approve_words(c: &Card, config: &mork_types::config::Config) -> String {
    let sketch = c.sketch.as_ref().map(mork_types::card::Sketch::words).unwrap_or_default();
    let link = super::mock::link(c, config).filter(|_| super::mock::briefing(c));
    let mock = link.map(|l| format!("\n\nmock: {l}")).unwrap_or_default();
    match sketch.trim() {
        "" => format!("approve the plan{mock}"),
        sketch => format!("approve the plan\n\n{sketch}{mock}"),
    }
}

// Whether a message is addressed to the agent holding the specification, by its name or its key.
fn to_it(who: &Who, card: &Card, m: &Message) -> bool {
    let name = who.role.as_str();
    m.to.as_deref().is_some_and(|t| t == name || t == format!("{}/{name}", card.id))
}

// An orchestrator's answer to the spec holder's own unanswered mechanical request keeps its recap (fac-649).
fn answering_request(state: &State, card: &Card, who: &Who, from: &Sender, intent: Option<MessageIntent>) -> bool {
    matches!(from, Sender::Orchestrator(project) if *project == card.project)
        && matches!(intent, None | Some(MessageIntent::Response))
        && crate::agents::stalled::unanswered_orchestrator_request(state, card, &who.key, who.place.as_str()).is_some()
}

/// Whether words to the card's specification holder withdraw its standing recap: the operator's always; the card's
/// project's orchestrator's only while its plan ask is given and not answering a mechanical request (fac-391, fac-649).
pub fn withdraws(state: &State, card: &Card, who: &Who, from: &Sender, intent: Option<MessageIntent>) -> bool {
    *from == Sender::Operator
        || (given_plan_holder(state, from, card) && !answering_request(state, card, who, from, intent))
}

/// Whether `a` is an open `approve-brief` ask of `card` currently given to an orchestrator.
pub(crate) fn is_given_plan(a: &mork_types::ask::Ask, card: &CardId) -> bool {
    a.open && a.given && a.kind == AskKind::ApproveBrief && a.card.as_ref() == Some(card)
}

/// Whether `from` is `card`'s project's orchestrator holding an open, given `approve-brief` ask for `card`.
pub(crate) fn given_plan_holder(state: &State, from: &Sender, card: &Card) -> bool {
    matches!(from, Sender::Orchestrator(p) if *p == card.project)
        && !card.visual
        && state.asks.values().any(|a| is_given_plan(a, &card.id))
}

/// Closes an open given `approve-brief` ask when its project's orchestrator writes to the spec holder in its room,
/// unless it is answering the spec holder's own unanswered mechanical request (fac-649).
pub(crate) fn close_given_plan(
    p: &mut Poster<'_>,
    who: &Who,
    from: &Sender,
    intent: Option<MessageIntent>,
) -> Vec<mork_types::AskId> {
    let Some(id) = places::card_of(p.state, &who.place) else { return Vec::new() };
    let Some(c) = p.card(&id) else { return Vec::new() };
    if who.role != super::facts::holder(&c)
        || who.place.as_str() != places::room_of(&id, who.role)
        || !given_plan_holder(p.state, from, &c)
        || answering_request(p.state, &c, who, from, intent)
    {
        return Vec::new();
    }
    let open: Vec<_> = p.all_asks().filter(|a| is_given_plan(a, &id)).cloned().collect();
    let mut ids = Vec::new();
    for mut a in open {
        ids.push(a.id.clone());
        a.close();
        a.answered_ms = Some(p.now);
        p.put_ask(a);
    }
    ids.sort();
    ids
}

// Whether a recap was made without the operator's or a delegated answer's latest word to its agent (fac-16, fac-649):
// authoritative words reached it, in its own room or its DM, after its turn's news was read, and weren't taken.
fn outdated(p: &Poster<'_>, who: &Who, card: &Card) -> bool {
    let Some(a) = p.agent(&who.key) else { return false };
    let (room, dm) = (places::room_of(&card.id, who.role), format!("{}/{}/dm", card.id, who.role.as_str()));
    let unread = |m: &Message| m.seq >= a.seen && !a.taken.contains(&m.seq);
    let authoritative = |m: &Message| m.from == Sender::Operator || (m.from.is_orchestrator() && m.delegated.is_some());
    [room.as_str(), dm.as_str()]
        .iter()
        .any(|place| p.state.messages_in(place).any(|m| unread(m) && authoritative(m) && to_it(who, card, m)))
}

/// Withdraws a waiting recap at `brief` and closes its `approve-brief` ask; mutates `c` in place and returns
/// whether `c` changed for its caller to stage (fac-16, fac-644).
pub fn redraft(p: &mut Poster<'_>, c: &mut Card) -> bool {
    if c.stage != Stage::Brief {
        return false;
    }
    asks::close(p, &places::room_of(&c.id, super::facts::specifier(c)), &[AskKind::ApproveBrief]);
    if c.draft.is_some() || c.recap || c.challenge.is_some() {
        c.draft = None;
        c.recap = false;
        c.challenge = None;
        return true;
    }
    false
}

// A stopped brief job can outlive approval and a new turn under its old key; returns whether any stopped.
pub(crate) fn stop_brief_turns(p: &mut Poster<'_>, key: &AgentKey) -> bool {
    let ending = p.ending.clone();
    let mut stopped = false;
    for j in p
        .state
        .jobs
        .values()
        .filter(|j| j.owner == *key && j.purpose.is_none() && j.settled.is_none() && ending.as_ref() != Some(&j.id))
    {
        let mut j = j.clone();
        j.stop_requested = true;
        j.detached = true;
        p.intent(Intent::StopJob { id: j.id.clone() });
        p.other.push(Change::PutJob(Box::new(j)));
        stopped = true;
    }
    stopped
}

/// Recovers a missing foreman's challenge of an unoffered plan.
pub fn recover_flow(p: &mut Poster<'_>, config: &mork_types::config::Config) {
    let ids: Vec<CardId> = p.state.cards.keys().cloned().collect();
    for id in ids {
        let Some(mut c) = p.card(&id).filter(|c| c.mode == Mode::Full) else { continue };
        let foreman = crate::messages::requests::agent_of(&id, Role::Foreman).key;
        if c.stage == Stage::Brief
            && c.draft.is_some()
            && !c.recap
            && p.agent(&foreman).is_none()
            && crate::agents::required::challenge(p, &mut c, config)
        {
            p.put_card(c);
        }
    }
}
