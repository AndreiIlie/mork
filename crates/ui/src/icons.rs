//! The icons (5.2): one Lucide glyph per meaning, in a tone (fac-103); and the agents' lights,
//! plain squares.

use iced::widget::canvas::{self, Canvas, Geometry};
use iced::widget::text;
use iced::widget::text::{LineHeight, Wrapping};
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::spans::Tone;
use crate::{fonts, theme};

/// An icon's side, in px: Lucide's glyphs fill their em, so 12 px draws them a 1 px stroke.
pub const SIZE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// A room's door.
    Door,
    /// An agent, still.
    Agent,
    /// An agent working.
    Working,
    /// A raised hand.
    Hand,
    /// A section of the side column, open or folded (fac-105).
    Unfolded,
    Folded,
    /// A page that opens outside the window: a card's pull request.
    Link,
    /// A step of an agent's list, done or still to come (fac-128).
    Done,
    Pending,
}

impl Icon {
    /// The glyph's code point in Lucide's font: door-closed, bot, loader-circle, hand, fishing-hook,
    /// chevron-down, chevron-right, external-link, circle-check, circle-dashed.
    pub fn glyph(self) -> char {
        match self {
            Icon::Door => '\u{e3d5}',
            Icon::Agent => '\u{e1bb}',
            Icon::Working => '\u{e10a}',
            Icon::Hand => '\u{e1d7}',
            Icon::Unfolded => '\u{e06d}',
            Icon::Folded => '\u{e06f}',
            Icon::Link => '\u{e0b9}',
            Icon::Done => '\u{e226}',
            Icon::Pending => '\u{e4b0}',
        }
    }
}

/// An icon's widget: its glyph in a tone, `SIZE` px square.
pub fn icon<'a, M: 'a>(icon: Icon, tone: Tone) -> Element<'a, M> {
    tinted(icon, theme::color(tone))
}

/// An icon in a colour of its own: a tone faded at the plan's edges (fac-128).
pub fn tinted<'a, M: 'a>(icon: Icon, color: Color) -> Element<'a, M> {
    text(icon.glyph())
        .font(fonts::ICONS)
        .size(SIZE)
        .line_height(LineHeight::Absolute(Pixels(SIZE)))
        .wrapping(Wrapping::None)
        .color(color)
        .width(Length::Fixed(SIZE))
        .height(Length::Fixed(SIZE))
        .into()
}

/// The working glyph, turning once a second on the window's animation clock (fac-110).
pub fn working<'a, M: 'a>(tone: Tone, now_ms: i64) -> Element<'a, M> {
    working_tinted(theme::color(tone), now_ms)
}

/// The working glyph turning, in a colour of its own (see [`tinted`]).
pub fn working_tinted<'a, M: 'a>(color: Color, now_ms: i64) -> Element<'a, M> {
    // why: the remainder is 0..1000, exactly representable in f32.
    #[allow(clippy::cast_precision_loss)]
    let angle = now_ms.rem_euclid(1_000) as f32 * std::f32::consts::TAU / 1_000.0;
    Canvas::new(Spinner { color, angle }).width(Length::Fixed(SIZE)).height(Length::Fixed(SIZE)).into()
}

#[derive(Debug, Clone, Copy)]
struct Spinner {
    color: Color,
    angle: f32,
}

impl<M> canvas::Program<M> for Spinner {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        frame.translate(iced::Vector::new(center.x, center.y));
        frame.rotate(self.angle);
        canvas::Text {
            content: Icon::Working.glyph().to_string(),
            position: Point::new(-SIZE / 2.0, -SIZE / 2.0),
            color: self.color,
            size: Pixels(SIZE),
            line_height: LineHeight::Absolute(Pixels(SIZE)),
            font: fonts::ICONS,
            ..canvas::Text::default()
        }
        .draw_with(|path, color| frame.fill(&path, color));
        vec![frame.into_geometry()]
    }
}

/// A light square (5.2): a 4×4 solid, with no glow (fac-103).
pub fn light<'a, M: 'a>(tone: Tone) -> Element<'a, M> {
    Canvas::new(Square { tone }).width(Length::Fixed(4.0)).height(Length::Fixed(4.0)).into()
}

#[derive(Debug, Clone, Copy)]
struct Square {
    tone: Tone,
}

impl<M> canvas::Program<M> for Square {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, Size::new(4.0, 4.0), theme::color(self.tone));
        vec![frame.into_geometry()]
    }
}
