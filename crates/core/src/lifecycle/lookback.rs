//! Look-back report cards have no board file; each run owns its range until its completion advances the cursor.

use mork_types::ask::AskKind;
use mork_types::card::{Card, Mode};
use mork_types::lookback::{Cursor, INPUT_FILE, Read, Run};
use mork_types::{CardId, ProjectId, Sha};
use serde_json::{Value, json};

use crate::decide::{Ctx, Decision};
use crate::post::Poster;
use crate::state::{Change, State};

const WEEK_MS: i64 = 7 * 86_400_000;

/// A project's next look-back card, numbered from 1 over its cards loaded or not: each has a room of its own.
pub fn next_id(state: &State, project: &str) -> CardId {
    let prefix = format!("lookback-{project}-");
    let taken = state.cards.keys().chain(state.unloaded.cards.iter());
    let top = taken.filter_map(|id| id.as_str().strip_prefix(&prefix)?.parse::<u64>().ok()).max().unwrap_or(0);
    CardId::new(format!("{prefix}{}", top + 1))
}

/// The project's live look-back ID and read, including reports started under the old IDs.
pub fn live<'a>(state: &'a State, project: &ProjectId) -> Option<(&'a CardId, Read)> {
    state
        .cards
        .values()
        .filter(|c| c.project == *project && c.stage.is_live())
        .find_map(|c| c.lookback.as_ref().map(|run| (&c.id, run.read)))
}

/// Starts a report on the next fetched base in a card of its own, refusing a second live read of the same project.
pub(super) fn start(state: &State, project: &ProjectId, read: Option<&str>, ctx: Ctx<'_>) -> Decision {
    let name = read.unwrap_or(Read::Pulse.spec().name);
    let Some(read) = Read::named(name) else {
        return Decision::refused(format!("unknown look-back read `{name}`"));
    };
    if !ctx.config.projects.contains_key(project) {
        return Decision::refused(format!("no project `{project}`"));
    }
    if let Some((id, _)) = live(state, project) {
        return Decision::refused(format!("{id} is still running"));
    }
    let id = next_id(state, project.as_str());
    let window = window(state, project, read, ctx.now);
    let title = format!("{} on {project}", read.spec().button);
    let mut p = Poster::for_ctx(state, ctx);
    let mut c = Card::new(id.clone(), project.as_str(), &title, Mode::Report, 1, ctx.now);
    c.delegated = crate::messages::auto::on(state, project);
    c.port = super::start::port(&p, ctx);
    c.description = description(&window, ctx.now, false);
    c.lookback = Some(window);
    c.first_seq = state.next_seq;
    p.put_card(c);
    p.touch_place(id.as_str());
    p.journal("card.start", Some(id.as_str()), json!({"run": 1, "mode": "report"}));
    p.fetch(project.as_str());
    let mut d = Decision::ok(json!({"lookback": {"card": id}}));
    p.finish(&mut d);
    d
}

fn previous<'a>(state: &'a State, project: &ProjectId, read: Read) -> Option<&'a Cursor> {
    state.lookbacks.get(project)?.get(&read)
}

fn window(state: &State, project: &ProjectId, read: Read, now: i64) -> Run {
    let mut run = Run { read, from: None, since_ms: None, ended_since_ms: None, through: None };
    if read == Read::Pulse {
        let last = previous(state, project, read);
        let week_ago = now.saturating_sub(WEEK_MS);
        run.from = last.and_then(|c| c.commit.clone());
        run.since_ms = run.from.is_none().then_some(week_ago);
        run.ended_since_ms = Some(last.and_then(|c| c.started_ms).unwrap_or(week_ago));
    }
    run
}

/// Pins the report's read to the commit selected for its clone, independently of later base refreshes.
pub fn fetched(card: &mut Card, tip: &Sha) {
    let Some(run) = card.lookback.as_mut().filter(|r| r.through.is_none()) else { return };
    run.through = Some(tip.clone());
    card.description = description(run, card.started_ms, false);
}

pub(super) fn cloned(card: &mut Card, result: &Value) {
    let (Some(run), Some(inputs)) = (card.lookback.as_mut(), result.get("lookback")) else { return };
    if let Some(from) = inputs.get("from").and_then(Value::as_str) {
        run.from = Some(Sha::new(from));
    }
    card.description = description(run, card.started_ms, true);
}

/// Records a completion once a report-ready reply has opened the card's Done ask.
pub(super) fn completed(p: &mut Poster<'_>, card: &Card) {
    let Some(run) = card.lookback.as_ref() else { return };
    if !p.all_asks().any(|ask| ask.open && ask.kind == AskKind::Done && ask.card.as_ref() == Some(&card.id)) {
        return;
    }
    let commit = run.through.clone().or_else(|| previous(p.state, &card.project, run.read)?.commit.clone());
    let cursor = Cursor {
        project: card.project.clone(),
        commit,
        started_ms: Some(card.started_ms),
        ended_ms: p.now,
        read: run.read,
    };
    p.journal("lookback.done", Some(card.id.as_str()), json!({"cursor": cursor}));
    p.other.push(Change::PutLookback(cursor));
}

fn description(run: &Run, started_ms: i64, prepared: bool) -> String {
    let through = run.through.as_ref().map(Sha::as_str).unwrap_or("pending clone");
    let spec = run.read.spec();
    let scope = if let Some(ended_since) = run.ended_since_ms {
        let merged = merged_scope(run, through);
        format!(
            "{merged} Read the cards that ended from {} up to {} (the start of this run).",
            stamp(ended_since),
            stamp(started_ms),
        )
    } else {
        format!("Run the `{}` read at `{through}`.", spec.name)
    };
    let mut inputs = String::new();
    if prepared {
        inputs = format!("\n\nRead the run's inputs in `{INPUT_FILE}`.");
        if run.read == Read::Pulse && run.from.is_none() {
            inputs.push_str(" No recorded landing supplies the starting commit. Use the skill's cutoff fallback.");
        }
    }
    format!(
        "{scope}{inputs}\n\nThe commit is fixed for this run. Load the `looking-back` skill and follow its `{}` recipe. \
         Leave grouped proposals and a short report. End with `report-ready`. The operator's `done` records the run once the report is ready.",
        spec.name,
    )
}

fn merged_scope(run: &Run, through: &str) -> String {
    match &run.from {
        Some(from) => format!("Read every change in `{from}..{through}`, carded or not."),
        None => {
            let since = run.since_ms.unwrap_or_default();
            format!(
                "Read every change that landed on the base since {} (epoch milliseconds: {since}), through `{through}`. \
                 Count when changes landed, never their commits' own dates.",
                stamp(since)
            )
        }
    }
}

fn stamp(ms: i64) -> String {
    let (y, mo, d, h, mi, s) = crate::state::names::civil_from_ms(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}
