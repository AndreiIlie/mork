//! The cards a run carries (fac-563): which run holds a card, who may join one, and the status writes each carried
//! card's board gets with its run's.

use factory_types::action::{Action, ActionStatus, ActionWhat, BoardEdit};
use factory_types::card::{BoardCard, Card, Carried, Mode, Stage};
use factory_types::{ActionId, CardId, ProjectId};

use crate::decide::{Ctx, Intent};
use crate::post::Poster;
use crate::state::State;

/// The run carrying a card among those `keep` accepts: the one membership read its reservation, its edits'
/// announcements and their notes' preload share.
pub fn carrying<'s>(state: &'s State, id: &CardId, keep: impl Fn(&Card) -> bool) -> Option<&'s Card> {
    state.cards.values().find(|c| c.carries.iter().any(|k| k.id == *id) && keep(c))
}

/// The run that holds a card it carries, other than `except`.
pub fn carrier<'s>(state: &'s State, id: &CardId, except: Option<&CardId>) -> Option<&'s Card> {
    carrying(state, id, |c| Some(&c.id) != except && holding(state, c))
}

/// Whether a run holds the cards it carries: while it is live, or closed with work that hasn't landed until a read
/// finds its own card closed on its board. What a start loads of a closed run is read by it too.
pub fn holding(state: &State, c: &Card) -> bool {
    c.stage.is_live() || (unlanded(state, c) && !c.released)
}

/// Whether a closed run left work that hasn't landed: a head or a publication, or its last bundle's head not yet
/// taken, whether that bundle still runs or its take is held.
pub fn unlanded(state: &State, c: &Card) -> bool {
    let saved = c.bundled.as_ref().is_some_and(|b| b.head.is_some());
    let made = c.head.is_some() || saved || c.continues.is_some() || c.pushed.is_some() || c.pr.is_some();
    c.stage == Stage::Closed && (made || crate::agents::closing::clone_going(state, c.id.as_str()))
}

/// Whether the boards' last read found a card closed on its board: a run closing then releases what it carries.
pub(super) fn closed_on_board(state: &State, id: &CardId) -> bool {
    state.board.get(id).is_some_and(closed)
}

// Only a read that finds the card closed releases what it carries: one that misses its file proves nothing.
fn closed(b: &BoardCard) -> bool {
    !b.file.starts_with("tasks/")
}

/// A boards read that finds a closed run's card closed releases what it carries for good: a later read that misses
/// its file, or a restart, holds nothing again.
pub fn released(p: &mut Poster<'_>, read: &[BoardCard]) {
    let found = |c: &Card| read.iter().any(|b| b.id == c.id && b.project == c.project && closed(b));
    let ids: Vec<CardId> = p
        .state
        .cards
        .values()
        .filter(|c| c.stage == Stage::Closed && !c.carries.is_empty() && !c.released && found(c))
        .map(|c| c.id.clone())
        .collect();
    for id in ids {
        let Some(mut c) = p.card(&id) else { continue };
        c.released = true;
        p.put_card(c);
    }
}

/// Why a card is held by another run, if it is: carried by one (`except` aside), or its `merged` write from a
/// run's landing not yet done, which no later run of that card releases.
pub fn taken(state: &State, id: &CardId, except: Option<&CardId>) -> Option<String> {
    if let Some(c) = carrier(state, id, except) {
        return Some(if c.stage.is_live() {
            format!("{id} is carried by {}'s run, at {}", c.id, c.stage.as_str())
        } else {
            format!(
                "{id} is carried by {}'s closed run, whose work hasn't landed: start {} again or close it",
                c.id, c.id
            )
        });
    }
    merging(state, id).map(|run| format!("{id}'s `merged` status from {run}'s landing isn't written yet"))
}

// The run whose landing's `merged` write to this card is still to be done: the write names the card it lands.
fn merging(state: &State, id: &CardId) -> Option<CardId> {
    state.actions.values().filter(|a| unsettled(a.status)).find_map(|a| {
        let ActionWhat::ChangeBoardCard { card: Some(run), lands: Some(k), .. } = &a.what else { return None };
        (k == id && run != id).then(|| run.clone())
    })
}

fn unsettled(s: ActionStatus) -> bool {
    matches!(s, ActionStatus::Pending | ActionStatus::Started | ActionStatus::Held)
}

/// The cards a start carries, with their words as their boards hold them, or why one of them can't join.
pub(super) fn members(
    p: &Poster<'_>,
    id: &CardId,
    mode: Mode,
    carries: &[CardId],
    prior: Option<&Card>,
    ctx: Ctx<'_>,
) -> Result<Vec<Carried>, String> {
    if !carries.is_empty() && mode != Mode::Full {
        return Err("only a full run carries other cards: start this one full, or the others on their own".into());
    }
    // Work that hasn't landed goes on only with the cards it carried: its branch never lands another set.
    let before = prior.filter(|c| unlanded(p.state, c)).map(|c| c.carries.iter().map(|k| &k.id).collect::<Vec<_>>());
    if let Some(before) = &before
        && (before.len() != carries.len() || carries.iter().any(|k| !before.contains(&k)))
    {
        return Err(format!("{id}'s earlier run left work that hasn't landed: start it again {}", with(before)));
    }
    // A new run would replace the clone whose last head that take keeps, and its retry would then be superseded.
    if let (Some(before), Some(c)) = (&before, prior)
        && !before.is_empty()
        && let Some(act) = held_take(p, c)
    {
        return Err(format!(
            "{id}'s last head isn't kept: its take {act} is held; retry it, then start {id} {}",
            with(before)
        ));
    }
    let mut out: Vec<Carried> = Vec::new();
    for k in carries {
        let joined = joins(p, id, k, &out, ctx);
        out.push(joined.map_err(|e| before.as_ref().map_or(e.clone(), |b| format!("{e}; {id} goes on {}", with(b))))?);
    }
    Ok(out)
}

// The held take of a closed run's last bundle: the one take whose retry keeps that run's head.
fn held_take(p: &Poster<'_>, c: &Card) -> Option<ActionId> {
    let b = c.bundled.as_ref()?;
    p.all_actions().find_map(|a| {
        let ActionWhat::Take { card, head, job } = &a.what else { return None };
        let last = *card == c.id && super::gates::last_bundle(b, job, head);
        (last && a.status == ActionStatus::Held).then(|| a.id.clone())
    })
}

fn with(cards: &[&CardId]) -> String {
    match cards {
        [] => "carrying no other card".to_owned(),
        _ => format!("carrying {}", cards.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(" ")),
    }
}

// One carried card read from its board, or why it can't join the run: each refusal makes nothing.
fn joins(p: &Poster<'_>, run: &CardId, k: &CardId, so_far: &[Carried], ctx: Ctx<'_>) -> Result<Carried, String> {
    let bc = super::start::runnable(p, k, ctx)?;
    if k == run || so_far.iter().any(|c| c.id == *k) {
        return Err(format!("{k} is named twice"));
    }
    let project = p.state.board.get(run).map(|b| &b.project);
    if project != Some(&bc.project) {
        return Err(format!("{k} is on {}, not on {run}'s project", bc.project));
    }
    if p.card(k).is_some_and(|c| unlanded(p.state, &c)) {
        return Err(format!("{k}'s own earlier run left work that hasn't landed: start {k} itself to go on with it"));
    }
    if let Some(why) = taken(p.state, k, Some(run)) {
        return Err(why);
    }
    Ok(Carried {
        id: k.clone(),
        title: bc.title.clone(),
        description: bc.description.clone(),
        acceptance: bc.acceptance.clone(),
        file: bc.file.clone(),
    })
}

/// Writes a status to a card's board on its run's behalf: the run's own card, or one it carries, whose write is
/// held, retried and asked about in the run's room. A landing's write to a carried card names that card.
pub(crate) fn status_write(
    p: &mut Poster<'_>,
    run: &Card,
    of: &CardId,
    status: &str,
    landing: bool,
) -> Option<ActionId> {
    let (project, file) = board_file(p.state, run, of)?;
    let edit = BoardEdit::Fields {
        title: None,
        status: Some(status.to_owned()),
        priority: None,
        labels: None,
        parent: None,
        acceptance: None,
        description: None,
        note: None,
    };
    let mut what = crate::board::writes::change(p.state, &project, &file, edit, Some(run.id.clone()), p.now);
    if let ActionWhat::ChangeBoardCard { lands, .. } = &mut what {
        *lands = (landing && *of != run.id).then(|| of.clone());
    }
    Some(own(p, run, what))
}

// Where a card's status goes: its file as the boards were last read, or for a card the run carries, the file it
// joined from when that read missed it.
fn board_file(state: &State, run: &Card, of: &CardId) -> Option<(ProjectId, String)> {
    if let Some(bc) = state.board.get(of) {
        return Some((bc.project.clone(), bc.file.clone()));
    }
    run.carries.iter().find(|k| k.id == *of && !k.file.is_empty()).map(|k| (run.project.clone(), k.file.clone()))
}

// The run's own write, made now or this card's still unsettled from an earlier run: one done already, or made for
// another card, proves nothing of this run, so its awaited ids bind its writes to it whatever the clock says.
fn own(p: &mut Poster<'_>, run: &Card, what: ActionWhat) -> ActionId {
    let base = what.key();
    let alike = |a: &Action| a.key == base || a.key.starts_with(&format!("{base}@"));
    let mine = |a: &Action| a.what.card() == Some(&run.id) && lands(&a.what) == lands(&what);
    let Some(a) = p.all_actions().find(|a| alike(a) && mine(a) && unsettled(a.status)).cloned() else {
        return p.act_as(what, true);
    };
    let id = a.id.clone();
    adopt(p, a);
    id
}

fn lands(what: &ActionWhat) -> Option<&CardId> {
    if let ActionWhat::ChangeBoardCard { lands, .. } = what { lands.as_ref() } else { None }
}

// A close cancels a run's pending writes and closes its asks: one a later run needs runs again, or asks again.
fn adopt(p: &mut Poster<'_>, mut a: Action) {
    let asked = p.all_asks().any(|x| x.open && x.action.as_ref() == Some(&a.id));
    if a.status == ActionStatus::Pending && a.cancelled {
        a.cancelled = false;
        let id = a.id.clone();
        p.put_action(a);
        p.intent(Intent::RunAction { id });
    } else if a.status == ActionStatus::Held && !asked {
        let why = a.words.clone().unwrap_or_default();
        crate::decide::actions::hold(p, a, &why);
    }
}

/// The run's own card and each it carries, in order.
pub fn cards(c: &Card) -> impl Iterator<Item = &CardId> {
    std::iter::once(&c.id).chain(c.carries.iter().map(|k| &k.id))
}
