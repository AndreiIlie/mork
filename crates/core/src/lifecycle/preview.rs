//! A full card's preview worker (fac-196): it runs only once the operator turned it on, with `preview` or `demo` (fac-222),
//! and its foreman may restart it while on (fac-348); the engine runs the worker, this records the call.

use mork_types::CardId;
use mork_types::card::{Card, Mode, Stage};
use mork_types::config::Config;
use mork_types::reply::Role;
use mork_types::state::{Agent, Sender};
use serde_json::json;

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::State;

/// The worker's output, in the clone where git doesn't see it and its seats can read it.
pub const LOG: &str = ".git/mork-preview.log";

/// What `preview` asks of a card's worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    /// `start`, `stop` or `restart`; none restarts it, which starts it when it doesn't run.
    pub fn parse(word: Option<&str>) -> Result<Self, String> {
        match word {
            Some("start") => Ok(Self::Start),
            Some("stop") => Ok(Self::Stop),
            Some("restart") | None => Ok(Self::Restart),
            Some(other) => Err(format!("`{other}` isn't a preview action: start, stop or restart")),
        }
    }
}

/// The project's preview command and the card's port, when the card
/// serves a preview: a live full card past `starting`, its clone made and installed.
pub fn serves<'a>(card: &Card, config: &'a Config) -> Option<(&'a str, u16)> {
    let project = config.projects.get(card.project.as_str())?;
    let command = project.preview.as_deref()?;
    let live = card.stage.is_live() && card.stage != Stage::Starting;
    (card.mode == Mode::Full && live).then_some(())?;
    Some((command, card.port?))
}

/// Every card whose preview should run, the operator having turned it on, with its command and port.
pub fn wanted<'a>(state: &State, config: &'a Config) -> Vec<(CardId, &'a str, u16)> {
    let on = state.cards.values().filter(|c| c.preview);
    on.filter_map(|c| serves(c, config).map(|(cmd, port)| (c.id.clone(), cmd, port))).collect()
}

/// A card's preview link: its own host name, so its login cookie is its own (fac-222).
pub(crate) fn url(card: &CardId, port: u16) -> String {
    format!("http://{}.localhost:{port}/", card.as_str().to_lowercase())
}

// The card `preview` or `demo` (`demo`) names, when it serves a preview it may be asked of.
fn servable<'a>(state: &'a State, card: &str, config: &Config, demo: bool) -> Result<&'a Card, String> {
    let c = state.cards.get(&CardId::new(card)).ok_or_else(|| format!("no card {card}"))?;
    if config.projects.get(c.project.as_str()).and_then(|p| p.preview.as_ref()).is_none() {
        return Err(format!("{card}'s project `{}` names no `preview` command", c.project));
    }
    if serves(c, config).is_none() {
        let stage = c.stage.as_str();
        return Err(format!("{card} serves no preview: only a live full card does, once its clone is made ({stage})"));
    }
    if demo && c.stage == Stage::Brief {
        return Err(format!("{card} is at brief: nothing is built yet to demo"));
    }
    Ok(c)
}

/// `preview <card> [action]`, or `demo <card>` (`demo`), past the buzz: the operator's call, or its foreman's restart of a
/// preview that is on, recorded on the card and said in its places, and a demo's reply asked of its foreman.
pub(super) fn command(
    state: &State,
    from: &Sender,
    card: &CardId,
    action: Option<&str>,
    demo: bool,
    ctx: Ctx<'_>,
) -> Decision {
    let action = match Action::parse(action) {
        Ok(a) => a,
        Err(e) => return Decision::invalid(e),
    };
    let id = card.as_str().to_lowercase();
    let mut c = match servable(state, &id, ctx.config, demo) {
        Ok(c) => c.clone(),
        Err(e) => return Decision::refused(e),
    };
    let Some((_, port)) = serves(&c, ctx.config) else { return Decision::refused(format!("{id} has no port")) };
    let link = url(&c.id, port);
    // Past the buzz, anyone but the operator is the card's foreman restarting it.
    let foreman = *from != Sender::Operator;
    if foreman && !c.preview {
        return Decision::refused(format!(
            "{id}'s preview is off: turning it on is the operator's, his /preview or /demo."
        ));
    }
    c.preview = action != Action::Stop;
    let mut p = Poster::for_ctx(state, ctx);
    if foreman {
        let text = format!(
            "{id}/foreman restarted {id}'s preview: it serves the clone's current code at {link}, its output in `{LOG}`."
        );
        crate::messages::announce::to_card(&mut p, &c.id, &format!("{id}/foreman restarted the preview"), &text);
    } else if c.preview {
        let text = format!(
            "The operator turned {id}'s preview on: it serves the clone at {link}, its output in `{LOG}`. He may be looking at it."
        );
        crate::messages::announce::to_card(&mut p, &c.id, &format!("{id}'s preview is on"), &text);
    }
    if demo {
        let text = format!("The operator asked for a demo of {id} at {link}. {}", reply_request(&link));
        crate::messages::requests::ask_agent(&mut p, &c.id, Role::Foreman, "Prepare a demo", &text);
    }
    p.journal("preview", Some(&id), json!({"on": c.preview, "demo": demo, "from": from}));
    p.put_card(c);
    let name = if demo { "demo" } else { "preview" };
    let mut d = Decision::ok(json!({ name: {"card": id, "on": action != Action::Stop, "url": link, "log": LOG} }));
    p.finish(&mut d);
    d
}

/// What a demo asks of the foreman, its preview served at `link`.
pub(super) fn reply_request(link: &str) -> String {
    format!(
        "Put the direct link to the changed page on {link} and a brief reminder of the change in your flagged hand message. Skip the walkthrough. Use existing data when it shows the change. Create data only when the change needs it and no existing data shows it. You can use SQL, the server's API, or a browser on the preview."
    )
}

/// A card seat's line on its preview, at its session's start; none for a card that serves none.
pub fn line(state: &State, a: &Agent, config: &Config) -> Option<String> {
    let id = crate::messages::places::card_of(state, &a.place)?;
    let c = state.cards.get(&id)?;
    let project = config.projects.get(c.project.as_str())?;
    let command = project.preview.as_deref()?;
    let port = c.port.filter(|_| c.mode == Mode::Full)?;
    Some(format!(
        "{id}'s preview runs only on the operator's call, his /preview or /demo in its room: the engine then runs `{command}` in its clone and serves it at {} (`MORK_PORT`), its output in `{LOG}`. No seat starts or stops it; you may suggest one to him. While it is on, its foreman may restart it (`preview {id} restart`) to serve the clone's current code.",
        url(&id, port)
    ))
}
