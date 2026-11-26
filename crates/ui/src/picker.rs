//! An agent's picker (fac-96) shows its current and next settings and which changes start a fresh session.

use iced::widget::{Column, Row, row};
use iced::{Element, Length};
use mork_types::config::{Cli, Effort};
use mork_types::view::{AgentRow, ModelChoice, Setting};

use crate::app::{App, Effect, Message, Overlay};
use crate::fonts;
use crate::spans::Tone;
use crate::widgets::{boxed_button, txt, wrapped};

const NAME_CHARS: f32 = 12.0;

/// Half a pair the picker holds while the next turn's other half is off offer (fac-115).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub model: Option<ModelChoice>,
    pub effort: Option<Effort>,
}

/// The picker of the agent with this key, while the place shown has it or its card the session.
pub fn view<'a>(app: &'a App, agent: &str, draft: &'a Draft) -> Element<'a, Message> {
    let Some(a) = app.agent_row(agent) else {
        return txt("no longer in this place", Tone::Dim).into();
    };
    let next = a.pending.as_ref().or(a.setting.as_ref());
    let model = draft.model.as_ref().or(next.map(|s| &s.model));
    let effort = draft.effort.or(next.map(|s| s.effort));
    let card = app.place().is_some_and(|v| v.card.is_some());
    let efforts = efforts(a, effort, model);
    Column::new().push(settings(a)).push(models(a, model, card)).push(efforts).spacing(fonts::LINE).into()
}

impl App {
    /// A picker's click: sent with an offered half the picker holds, else alone while the next
    /// turn's other half stays on offer and runs with it, else held; neither half is filled in for the user.
    pub(crate) fn picked(&mut self, agent: &str, model: Option<ModelChoice>, effort: Option<Effort>) -> Vec<Effect> {
        let held = self.take_draft(agent);
        let Some(a) = self.agent_row(agent) else { return self.pick(agent, model, effort) };
        let offered = |m: Option<&ModelChoice>, e: Option<Effort>| {
            m.is_none_or(|m| a.choices.contains(m)) && e.is_none_or(|e| a.efforts.contains(&e))
        };
        if !offered(model.as_ref(), effort) {
            return self.pick(agent, model, effort);
        }
        let model = model.or(held.model.filter(|m| offered(Some(m), None)));
        let effort = effort.or(held.effort.filter(|e| offered(None, Some(*e))));
        let next = a.pending.as_ref().or(a.setting.as_ref());
        let alone =
            next.is_none_or(|s| offered(model.is_none().then_some(&s.model), effort.is_none().then_some(s.effort)));
        // Some models lack higher efforts: a seat standing at one is asked for an effort, not sent a refused pick.
        let pair = next.map(|s| (model.as_ref().unwrap_or(&s.model), effort.unwrap_or(s.effort)));
        let pair = pair.or(model.as_ref().zip(effort));
        let runs = pair.is_none_or(|(m, e)| m.cli.takes(&m.model, e));
        if runs && (alone || (model.is_some() && effort.is_some())) {
            return self.pick(agent, model, effort);
        }
        let (what, other) = match (&model, effort) {
            (Some(m), _) => (m.label.clone(), "an effort"),
            (None, e) => (e.map_or_else(String::new, |e| e.as_str().to_owned()), "a model"),
        };
        let why = match pair.filter(|_| !runs) {
            Some((m, e)) => format!("{} takes no {}", m.label, e.as_str()),
            None => "the next turn's isn't on offer".to_owned(),
        };
        self.note = Some(format!("{what} chosen: choose {other} too, {why}"));
        // A click the shown efforts no longer offer keeps the drafted model, its effort still to choose.
        let effort = effort.filter(|e| model.as_ref().is_none_or(|m| m.cli.takes(&m.model, *e)));
        if let Some(Overlay::Pick { agent: shown, draft }) = &mut self.overlay
            && shown == agent
        {
            *draft = Draft { model, effort };
        }
        Vec::new()
    }

    fn take_draft(&mut self, agent: &str) -> Draft {
        match &mut self.overlay {
            Some(Overlay::Pick { agent: shown, draft }) if shown == agent => std::mem::take(draft),
            _ => Draft::default(),
        }
    }
}

// Now and from its next turn, which a pick waits for.
fn settings<'a>(a: &AgentRow) -> Element<'a, Message> {
    let line = |s: &Setting| format!("{} · {} · {}", vendor(s.model.cli), s.model.label, s.effort.as_str());
    let mut col = Column::new();
    if let Some(s) = &a.setting {
        col = col.push(pair("now", txt(line(s), Tone::Text)));
    }
    let next = a.pending.as_ref().map_or_else(|| txt("the same", Tone::Dim), |p| txt(line(p), Tone::Orange));
    let why = "A pick applies from the agent's next turn: a turn running finishes as it launched.";
    col.push(pair("next turn", next)).push(wrapped(why, Tone::Dim)).into()
}

// The warnings refer to the session's selection, even when a different pick is pending.
fn models<'a>(a: &'a AgentRow, lit: Option<&ModelChoice>, card: bool) -> Element<'a, Message> {
    let runs = a.setting.as_ref().map(|s| &s.model);
    let mut clis: Vec<Cli> = Vec::new();
    for m in &a.choices {
        if !clis.contains(&m.cli) {
            clis.push(m.cli);
        }
    }
    clis.sort_by_key(|c| Some(*c) != runs.map(|m| m.cli));
    let reads = if card { " It reads the card first." } else { "" };
    let mut col = Column::new().push(txt("model", Tone::Dim));
    for cli in clis {
        let mut r = Row::new().spacing(fonts::ADVANCE * 2.0);
        for (i, m) in a.choices.iter().enumerate().filter(|(_, m)| m.cli == cli) {
            let on = lit == Some(m);
            let pick = Message::Pick { agent: a.agent.to_string(), model: Some(m.clone()), effort: None };
            r = r.push(boxed_button(format!("pickmodel:{i}"), m.label.clone(), tone(on), pick));
        }
        let mut cell = Column::new().push(r.wrap());
        let change = if runs.is_some_and(|m| m.cli != cli) { Some("Choosing another vendor") } else { None };
        if let Some(change) = change {
            let why = format!(
                "{change} starts a fresh session on the next turn.{reads} The room gets one line, and the \
                 earlier conversation stays."
            );
            cell = cell.push(wrapped(why, Tone::Orange));
        }
        col = col.push(pair(vendor(cli), cell));
    }
    col.into()
}

// Only what the drafted or selected model runs at.
fn efforts<'a>(a: &'a AgentRow, lit: Option<Effort>, model: Option<&ModelChoice>) -> Element<'a, Message> {
    let mut r = Row::new().spacing(fonts::ADVANCE * 2.0);
    for &e in a.efforts.iter().filter(|e| model.is_none_or(|m| m.cli.takes(&m.model, **e))) {
        let on = lit == Some(e);
        let pick = Message::Pick { agent: a.agent.to_string(), model: None, effort: Some(e) };
        r = r.push(boxed_button(format!("pickeffort:{}", e.as_str()), e.as_str().to_owned(), tone(on), pick));
    }
    pair("effort", r.wrap())
}

// What the next turn takes is lit as the open place's row is.
fn tone(on: bool) -> Tone {
    if on { Tone::White } else { Tone::Green }
}

fn pair<'a>(name: &'a str, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![txt(name, Tone::Dim).width(Length::Fixed(fonts::ADVANCE * NAME_CHARS)), value.into()].into()
}

fn vendor(cli: Cli) -> &'static str {
    match cli {
        Cli::Claude => "Claude",
        Cli::Codex => "Codex",
        Cli::Agy => "agy",
    }
}
