//! A card's controls (fac-474): binary active and visual tracks, and one three-position delegation control.
//! Each position takes a click only while the engine offers its command.

use iced::widget::{Column, container};
use iced::{Element, Length};
use mork_types::Command;
use mork_types::about::Switch;
use mork_types::view::PlaceView;

use crate::app::{App, Effect, Message};
use crate::spans::Tone;
use crate::widgets::{self, clickable, txt};
use crate::{input, theme};

const TRACK_W: f32 = 26.0;
const TRACK_H: f32 = 14.0;
const KNOB: f32 = 10.0;
const NAME_GAP: f32 = 8.0;
const GAP: f32 = 20.0;

/// The card's binary switches on the first row, then each multi-position control on its own row.
pub fn view<'a>(app: &App, v: &'a PlaceView) -> Element<'a, Message> {
    let switches = v.card.as_ref().map(|h| h.about.switches.as_slice()).unwrap_or_default();
    let mut line = Vec::new();
    let mut rows = Vec::new();
    for s in switches {
        let parts = vec![txt(s.name.as_str(), Tone::Dim).into(), widgets::gap(NAME_GAP), switch(app, s)];
        if s.positions.len() > 2 {
            rows.push(widgets::centred(parts).into());
            continue;
        }
        if !line.is_empty() {
            line.push(widgets::gap(GAP));
        }
        line.extend(parts);
    }
    Column::new().push(widgets::centred(line)).extend(rows).spacing(6.0).into()
}

fn switch<'a>(app: &App, s: &'a Switch) -> Element<'a, Message> {
    if s.positions.len() > 2 {
        return levels(app, s);
    }
    let on = s.position != 0;
    let command = s.positions.get(usize::from(!on)).and_then(|p| p.command.clone());
    let live = command.is_some();
    let knob = container(widgets::gap_v(KNOB)).width(KNOB).style(theme::switch_knob(on, live));
    let side = if on { iced::alignment::Horizontal::Right } else { iced::alignment::Horizontal::Left };
    let track = container(knob)
        .width(Length::Fixed(TRACK_W))
        .height(Length::Fixed(TRACK_H))
        .padding((TRACK_H - KNOB) / 2.0)
        .align_x(side)
        .align_y(iced::alignment::Vertical::Center)
        .style(theme::switch_track(on, live));
    let flip = command.map(|c| Message::Switch(app.session.place.clone(), Box::new(c)));
    clickable(format!("switch:{}", s.name), track, flip)
}

fn levels<'a>(app: &App, s: &'a Switch) -> Element<'a, Message> {
    let live = s.positions.iter().any(|p| p.command.is_some());
    let positions = s
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let selected = i == s.position;
            let tone = if selected { Tone::Green } else { Tone::Dim };
            let label = container(txt(p.label.as_str(), tone)).padding([1.0, 6.0]).style(move |_| container::Style {
                background: selected.then_some(theme::SELECTED.into()),
                ..container::Style::default()
            });
            let command = p.command.clone().map(|c| Message::Switch(app.session.place.clone(), Box::new(c)));
            clickable(format!("switch:{}:{i}", s.name), label, command)
        })
        .collect();
    container(widgets::centred(positions)).padding(2.0).style(theme::switch_track(false, live)).into()
}

impl App {
    /// A control's selection is sent only while the place's latest view offers that command.
    pub(crate) fn switch(&mut self, place: &str, command: &Command) -> Vec<Effect> {
        let about = self.places.get(place).and_then(|v| v.card.as_ref()).map(|h| &h.about);
        if !about
            .is_some_and(|a| a.switches.iter().flat_map(|s| &s.positions).any(|p| p.command.as_ref() == Some(command)))
        {
            self.note = Some(format!("✗ that switch no longer flips: {} not sent", command.name()));
            return Vec::new();
        }
        self.note = None;
        self.act_on(input::pressed(command))
    }
}
