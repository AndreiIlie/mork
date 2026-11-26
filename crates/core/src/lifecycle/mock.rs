//! A visual card's planner makes its mock in the spec conversation, on the baseline step's pages, and the plan's approve
//! ends it; the operator's reopen after it is a step of its own (fac-499, fac-591, fac-633, fac-636).

use mork_types::ask::{AskKind, Color};
use mork_types::card::{Card, MockStep, Mode, Stage};
use mork_types::config::Config;
use mork_types::reply::{Outcome, Reply, Role};
use mork_types::state::{Agent, Job, Owed, Sender};
use serde_json::json;

use crate::agents::prompt::held::{self, Sections};
use crate::decide::{Ctx, Decision};
use crate::messages::asks::{self, Raise};
use crate::messages::places::Who;
use crate::post::Poster;
use crate::state::State;

/// The mock's folder in the card's clone: untracked, never committed, kept aside while the clone is gone.
pub const FOLDER: &str = ".git/mork-mock";
/// The mock host's output, in the clone where git doesn't see it.
pub const LOG: &str = ".git/mork-mock-host/host.log";
/// The host's own files, its Vite configuration and cache, beside the mock and apart from it.
pub const HOST: &str = ".git/mork-mock-host";
/// The engine's request to the planner to make the mock: an offer counts only from a turn that read the latest.
pub(super) const HEADLINE: &str = "Mock the approved plan";

/// Whether a card's mock is open at build: the operator's reopen.
pub fn open(c: &Card) -> bool {
    c.stage == Stage::Build && c.mock.is_some_and(|m| m.step == MockStep::Open)
}

/// Whether a card's mock is at its baseline step, the planner's turns held (fac-636).
pub fn baseline(c: &Card) -> bool {
    c.mock.is_some_and(|m| m.step == MockStep::Baseline)
}

/// Whether the card's mock is open at brief (fac-636).
pub fn briefing(c: &Card) -> bool {
    c.stage == Stage::Brief && c.mock.is_some_and(|m| m.step == MockStep::Open)
}

/// Whether its planner writes the card's mock: open at brief beside the plan, or at build on a reopen.
pub fn writes(c: &Card) -> bool {
    open(c) || briefing(c)
}

/// Whether this seat's turns wait for its card's reopened mock: any foreman seat that is not the card's mock holder
/// (all foreman seats on a card with a planner, and the foreman's DM seat on a no-planner card) (fac-591, fac-688).
pub fn holds(state: &State, a: &Agent) -> bool {
    a.role == Role::Foreman
        && crate::messages::places::card_of(state, &a.place)
            .and_then(|id| state.cards.get(&id))
            .is_some_and(|c| open(c) && (!c.no_planner || a.place.as_str() != c.id.as_str()))
}

fn offered(c: &Card) -> bool {
    open(c) && c.mock.is_some_and(|m| m.offered)
}

/// Withdraws the open mock's offer when its holder starts a turn.
pub(crate) fn starting(p: &mut Poster<'_>, a: &Agent) {
    let card = crate::messages::places::card_of(p.state, &a.place).and_then(|id| p.card(&id));
    let Some(mut c) = card.filter(|c| {
        offered(c)
            && a.role == super::facts::specifier(c)
            && a.place.as_str() == crate::messages::places::room_of(&c.id, a.role)
    }) else {
        return;
    };
    if let Some(m) = c.mock.as_mut() {
        m.offered = false;
    }
    for ask in asks::open_of_card(p, c.id.as_str()).into_iter().filter(|a| a.kind == AskKind::Mock) {
        asks::close_one(p, &ask.id);
    }
    p.put_card(c);
}

/// The mock's link, while a Vite host is set and the card has its port.
pub(crate) fn link(c: &Card, config: &Config) -> Option<String> {
    config.mork.vite.as_ref()?;
    c.mock.and_then(|m| m.port).map(|port| super::preview::url(&c.id, port))
}

/// Every live card whose mock host should run, with its port: from its step's opening until the card
/// ends, unless visual mode turned off skipped it.
pub fn wanted(state: &State) -> Vec<(mork_types::CardId, u16)> {
    let live = state.cards.values().filter(|c| c.stage.is_live() && c.mode == Mode::Full);
    let kept = live.filter_map(|c| c.mock.filter(|m| m.step != MockStep::Skipped).map(|m| (c, m)));
    kept.filter_map(|(c, m)| Some((c.id.clone(), m.port?))).collect()
}

const OFFER: &str = "When the mock is ready, offer it with its link in your hand's message, asking nothing, and end `brief-ready`: that raises a mock ask in the card's spec room with an approve button, the operator's alone. Every turn of yours takes the offer down and closes the ask; approve is refused until you offer again. Your next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to you was left unread: that message already owes you another turn. While the mock is open his words in the spec room reach you, and those in the build room the foreman: change the mock and record each change he asks for as a decision on the card. The foreman neither offers the mock nor relays his comments. The plan is never recapped or challenged again. His approve ends the mock and asks the foreman for the build, or to continue it after a reopened mock, with no final recap. No build turn starts beside the mock: on a reopen, one already under way may finish meanwhile, without submitting, and you don't wait for it.";

const OFFER_ALONE: &str = "When the mock is ready, offer it with its link in your hand's message, asking nothing, and end `brief-ready`: that raises a mock ask in the card's room with an approve button, the operator's alone. Every turn of yours takes the offer down and closes the ask; approve is refused until you offer again. Your next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to you was left unread: that message already owes you another turn. While the mock is open his words in the card's room reach you: change the mock and record each change he asks for as a decision on the card. Edit no tracked repository files and do not submit `built` while the mock is open. The plan is never recapped again. His approve ends the mock and asks you to continue the build, with no final recap.";

pub(super) fn request(c: &Card, config: &Config) -> String {
    let id = &c.id;
    let served = served(c, config);
    if c.no_planner {
        return format!(
            "The operator reopened {id}'s approved mock: change it as he asks in the card's room. The build waits for the mock's approve, and `built` is refused until then. Make the mock in your clone's `{FOLDER}/` (untracked, never committed), {served}. Edit no tracked repository files while the mock is open. Keep any mock already there, an earlier run's included, and change it to the approved plan. Changes to the mock must follow the project's own written rules for its look. {OFFER_ALONE}"
        );
    }
    format!(
        "The operator reopened {id}'s approved mock: change it as he asks in the card's spec room. The build waits for the mock's approve: no foreman turn starts until then, and one under way finishes without submitting. Start the mock at once: don't wait for a turn under way, which doesn't use the mock's folder. Make the mock in your clone's `{FOLDER}/` (untracked, never committed), {served}. The rest of the clone is read-only to you. Keep any mock already there, an earlier run's included, and change it to the approved plan. Changes to the mock must follow the project's own written rules for its look. {OFFER}"
    )
}

/// Where the mock is served, or that nothing serves it and the seat raises that in its flagged hand message.
pub(super) fn served(c: &Card, config: &Config) -> String {
    match (link(c, config), config.mork.vite.is_some()) {
        (Some(link), _) => format!(
            "which the engine serves with Vite at {link}, its `index.html` first, the clone readable so it can import the project's SASS and files; its output in `{LOG}`"
        ),
        (None, true) => "but no port of Mork's range was free for its host, so nothing serves it: raise that in your flagged hand message".to_owned(),
        (None, false) => "but Mork has no Vite host set (mork.toml's `[mork] vite`), so nothing serves it: raise that in your flagged hand message".to_owned(),
    }
}

/// `approve` of a card in its mock step, past the buzz: the operator's approval of its offered mock.
pub(super) fn approve(state: &State, c: Card, ctx: Ctx<'_>) -> Decision {
    let mut p = Poster::for_ctx(state, ctx);
    let id = c.id.clone();
    if !offered(&c) {
        let who = if c.no_planner { "foreman" } else { "planner" };
        return Decision::refused(format!(
            "{id}'s mock waits for no approval yet: its {who} offers it with its link, asking nothing"
        ));
    }
    end(&mut p, c, MockStep::Approved);
    let mut d = Decision::ok(json!({"approve": {"card": id.as_str()}}));
    p.finish(&mut d);
    d
}

/// Ends a reopened mock at build with approval or visual mode off, and asks the foreman to continue the build.
pub(super) fn end(p: &mut Poster<'_>, mut c: Card, step: MockStep) {
    let id = c.id.clone();
    if let Some(m) = c.mock.as_mut() {
        m.step = step;
        m.offered = false;
    }
    c.draft = None;
    p.put_card(c.clone());
    for ask in asks::open_of_card(p, id.as_str()).into_iter().filter(|a| a.kind == AskKind::Mock) {
        asks::close_one(p, &ask.id);
    }
    p.journal("mock.ended", Some(id.as_str()), json!({"step": step}));
    let notes = c.mock.and_then(|m| m.reopened).unwrap_or(0);
    super::reopen::continued(p, c, notes, super::reopen::Back::Mock);
}

/// Takes the planner's clear mock offer, withholding one with unread messages: whether its reply offered.
pub(super) fn replied(p: &mut Poster<'_>, c: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    if !clear_offer(p, c, who, r, job) {
        return false;
    }
    // A missed message already owes another turn.
    if unread(p, who, job) {
        return true;
    }
    let first = !offered(c);
    let text = approval_text(c);
    let mut c = c.clone();
    c.draft = Some(r.hand.message.clone());
    if let Some(m) = c.mock.as_mut() {
        m.offered = true;
    }
    p.put_card(c);
    if let Some(mut a) = p.agent(&who.key) {
        held::wrote(&mut a, job, "offer", r.hand.message.trim());
        p.put_agent(a);
    }
    if first {
        let mut raise = Raise::new(who.place.as_str(), AskKind::Mock, Color::Yellow, text);
        raise.seq = p.posted_hand(&mork_types::state::Sender::Agent(who.key.clone()));
        raise.agent = Some(who.key.clone());
        asks::raise(p, raise);
    }
    true
}

/// Whether `r` is a valid clear reopened-mock offer from the card's mock holder in its room.
pub(super) fn clear_offer(p: &Poster<'_>, c: &Card, who: &Who, r: &Reply, job: &Job) -> bool {
    let holder = super::facts::specifier(c);
    who.role == holder
        && who.place.as_str() == crate::messages::places::room_of(&c.id, holder)
        && r.outcome == Outcome::BriefReady
        && open(c)
        && !r.hand.ask
        && r.hand.color != Color::Red
        && !r.hand.message.trim().is_empty()
        && read_request(p, who.place.as_str(), HEADLINE, 0, job)
}

// Only a turn still owed withholds the offer: a retry rereads its news and owes none (fac-514).
fn unread(p: &Poster<'_>, who: &Who, job: &Job) -> bool {
    let Some(a) = p.agent(&who.key) else { return false };
    matches!(a.owed, Some(Owed::Wake)) || a.wake_after || job.delivered.iter().any(|s| !a.taken.contains(s))
}

fn approval_text(c: &Card) -> String {
    let room = if c.no_planner { "room" } else { "spec room" };
    format!("approve {}'s mock if it looks right, or comment in its {room} to change it", c.id)
}

/// Whether a job read the engine's last request of `headline` in `place`, of seq `from` on, before it launched or
/// during it: an earlier turn cannot offer a later mock's work, nor end a later baseline step.
pub(super) fn read_request(p: &Poster<'_>, place: &str, headline: &str, from: u64, job: &Job) -> bool {
    let asks = |m: &&mork_types::state::Message| m.seq >= from && m.from == Sender::Engine && m.headline == headline;
    let asked = p.state.messages_in(place).filter(asks).last();
    asked.is_some_and(|m| {
        job.seen.is_some_and(|seen| m.seq < seen) || p.agent(&job.owner).is_some_and(|a| a.taken.contains(&m.seq))
    })
}

// The approved plan stays fixed; decisions requested at the mock augment it without another brief.
const STEP: &str = "## The mock\n\nThe plan is approved. Mock only the surfaces it names, following the project's written rules for its look, in the mock's folder: the rest of the clone is read-only to you. Offer the mock with its link, asking nothing, by ending `brief-ready`. Every turn of yours takes the offer down and closes its approve ask. Your next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to you was left unread: that message already owes you another turn. Approve is refused while the offer is down. Record changes the operator asks for as decisions on the card and make them in the mock. Never return to brief, recap the plan or have it challenged again. Only the operator's approve ends the mock, or visual mode turned off; either asks the foreman for the build, or to continue it, with no final recap. No build turn starts beside the mock: on a reopen, one already under way may finish meanwhile, without submitting, and you don't wait for it.";

const STEP_ALONE: &str = "## The mock\n\nThe plan is approved. Mock only the surfaces it names, following the project's written rules for its look, in the mock's folder: edit no tracked repository files while the mock is open, and `built` is refused until it ends. Offer the mock with its link, asking nothing, by ending `brief-ready`. Every turn of yours takes the offer down and closes its approve ask. Your next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to you was left unread: that message already owes you another turn. Approve is refused while the offer is down. Record changes the operator asks for as decisions on the card and make them in the mock. Never return to brief or recap the plan again. Only the operator's approve ends the mock, or visual mode turned off; either asks you to continue the build, with no final recap.";

/// The open mock's context for its holder, or a waiting foreman seat, sent when changed.
pub(crate) fn standing(c: &Card, holder: bool, s: &mut Sections<'_>, out: &mut String) {
    if !holder {
        let text = if c.no_planner {
            "\n## The mock\n\nThe card's room foreman makes and offers the mock, and takes the operator's comments in the card's room. You do not mock or relay comments. The operator reopened it: no new turn of yours starts until it ends, and `built` is refused. Wait for the mock's approval or visual mode off.\n"
        } else {
            "\n## The mock\n\nThe planner makes and offers the mock, and takes the operator's comments in the spec room. You do not mock or relay comments. The operator reopened it: no new turn of yours starts until it ends, and `built` is refused. Wait for the engine's request for the build, or to continue it, after the mock's approval or visual mode off.\n"
        };
        if s.sends("specification", text) {
            out.push_str(text);
        }
        return;
    }
    // The previous offer is the holder's own words, which its session holds once it wrote them (fac-623).
    let now = match c.draft.as_deref().map(str::trim) {
        Some(d) if s.sends("offer", d) => {
            format!("The mock is not offered during your turn. Its previous offer:\n\n{d}\n")
        }
        Some(_) | None => "The mock is not offered during your turn.\n".to_owned(),
    };
    let body = if c.no_planner { STEP_ALONE } else { STEP };
    let step = format!("\n{body}\n\n");
    out.push_str(if s.sends("specification", &step) { &step } else { "\n## The mock\n\n" });
    let waits = if c.no_planner {
        "the build waits for its approve, and `built` is refused until then"
    } else {
        "the build waits for its approve, and a build turn under way since before finishes without submitting: mock without waiting for it"
    };
    out.push_str(&format!("Where it stands: the operator reopened the approved mock; {waits}. {now}"));
}

/// The brief-time mock's rule for its specification holder, in its requests and its standing section.
pub(super) fn at_brief_rule(c: &Card) -> String {
    let freeze = if c.no_planner {
        "While the recap waits for approve, leave the mock untouched until the operator's comment withdraws it."
    } else {
        "While the recap is under challenge or waits for approve, leave the mock untouched until the foreman's \
         objection arrives or the operator's comment withdraws the recap."
    };
    format!(
        "Render and inspect the existing baseline. Raise the design direction, information hierarchy and concrete \
         changes in your design ask. Leave the mock untouched until the operator confirms them or, on a self-sustained \
         card, the orchestrator confirms them. On a delegated card, say the ask needs the operator's own answer. \
         Record the decisions and apply the confirmed changes together in the first draft. \
         Follow the `visual-mocks` skill and the project's written rules for its look. Confine mock \
         edits to `{FOLDER}/` and leave tracked files untouched before approval. Record each refinement as a decision \
         and render the mock from its link in the wall's headless Chromium before each recap. The recap is the plan \
         and the mock together. {freeze} Only the operator approves them. After approval, edit the mock only on his reopen."
    )
}

/// The brief-time mock's standing section for its specification holder, sent when changed.
pub(crate) fn at_brief(c: &Card, s: &mut Sections<'_>, out: &mut String) {
    let text = format!("\n## The mock\n\n{}\n", at_brief_rule(c));
    if s.sends("mock", &text) {
        out.push_str(&text);
    }
}

/// A visual card seat's line on its mock, at its session's start.
pub fn line(state: &State, a: &Agent, config: &Config) -> Option<String> {
    let id = crate::messages::places::card_of(state, &a.place)?;
    let c = state.cards.get(&id).filter(|c| c.visual && c.mode == Mode::Full)?;
    let served = match (link(c, config), config.mork.vite.is_some()) {
        (Some(link), _) => format!("serves it with Vite at {link} until the card ends, its output in `{LOG}`"),
        (None, true) => "serves it with Vite once its baseline step opens at brief".to_owned(),
        (None, false) => "serves nothing: Mork has no Vite host set (mork.toml's `[mork] vite`)".to_owned(),
    };
    let who = if c.no_planner { "foreman" } else { "planner" };
    Some(format!(
        "{id} is a visual card: at brief its {who} edits the baseline only after the visual direction is confirmed. The plan's approve includes the mock. The mock is kept in the clone's `{FOLDER}/`, untracked and never committed; an earlier run's mock is put back there when a later run's clone is made, and its baseline seat keeps it. The engine {served}. No seat starts or stops its host."
    ))
}

/// The mock's line in a review round's request, beside the specification its reviewers read.
pub fn for_review(c: &Card, config: &Config) -> String {
    let approved = c.visual && c.mock.is_some_and(|m| m.step == MockStep::Approved);
    let Some(link) = link(c, config).filter(|_| approved) else { return String::new() };
    format!(
        "\n\n{} is a visual card: its approved mock is served at {link}. Review the change against the specification and that mock. Judge the mock from a screenshot of that link rendered in the wall's headless Chromium, never from its source files: its own script may draw the approved state when the page runs. For the browser's launch, see \"Headless Chromium\" in the `visual-mocks` skill. Pass the link and this rule to each reviewer whose topic the mock bears on.",
        c.id
    )
}
