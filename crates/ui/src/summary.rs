// A card's room's summaries above its seats, the same in its spec and build rooms (fac-594, fac-633): its card's, then each carried card's under
// its id and title, in one box at the conversation's width that scrolls inside itself when tall.

use iced::widget::{Column, container};
use iced::{Element, Length, Padding};
use mork_types::view::{PlaceKind, PlaceView};

use crate::app::{App, Message};
use crate::conversation::{COLUMN_PAD, COLUMN_W};
use crate::spans::Tone;
use crate::widgets::{self, bold, gap_v};
use crate::{prose, theme, titles, wrappers};

const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 6.0;
const BETWEEN: f32 = 8.0;
const UNDER: f32 = 6.0;

/// The place's summaries in their box, at most `max_h` tall, or none when it has none.
pub fn view<'a>(app: &'a App, place: &'a PlaceView, max_h: f32) -> Option<Element<'a, Message>> {
    let (PlaceKind::Card { card } | PlaceKind::Spec { card }) = &place.kind else { return None };
    if place.summaries.is_empty() {
        return None;
    }
    let t = titles::of(&app.screen);
    let mut col = Column::new();
    for (i, s) in place.summaries.iter().enumerate() {
        if i > 0 {
            col = col.push(gap_v(BETWEEN));
        }
        if s.card != *card {
            col = col.push(bold(format!("{}: {}", s.card, s.title), Tone::Bright));
        }
        col = col.push(prose::view(&s.text, Tone::Text, &t));
    }
    let list = widgets::scroll(container(col).padding([PAD_Y, PAD_X])).height(Length::Shrink);
    let framed = container(list).max_height(max_h).width(Length::Fill).style(theme::field);
    let width = container(framed).max_width(COLUMN_W + 2.0 * COLUMN_PAD).padding([0.0, COLUMN_PAD]);
    let frame = container(width).center_x(Length::Fill).padding(Padding::ZERO.bottom(UNDER));
    Some(wrappers::room_summary(frame, card))
}
