//! The small widgets every pane uses: text in a tone, a line of spans, a clickable row with its
//! id, a button drawn as its words, an image's thumbnail. Every row is one line, 16 px (5.3.1).

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Row, Text, button, container, mouse_area, row, text};
use iced::{Element, Length, Pixels, mouse};

use crate::fonts;
use crate::spans::{Span, Tone};
use crate::theme;

/// Body text in a tone: 13 px on a 16 px line, not wrapped.
pub fn txt<'a>(s: impl text::IntoFragment<'a>, tone: Tone) -> Text<'a> {
    label(s).color(theme::color(tone))
}

// Body text in no tone of its own: a button's words take its style's, which brightens on hover.
fn label<'a>(s: impl text::IntoFragment<'a>) -> Text<'a> {
    text(s)
        .font(fonts::MONO)
        .size(fonts::SIZE)
        .line_height(LineHeight::Absolute(Pixels(fonts::LINE)))
        .wrapping(Wrapping::None)
}

pub fn bold<'a>(s: impl text::IntoFragment<'a>, tone: Tone) -> Text<'a> {
    txt(s, tone).font(fonts::MONO_BOLD)
}

/// Footer and hint text in a tone: 11 px on a 14 px line.
pub fn small<'a>(s: impl text::IntoFragment<'a>, tone: Tone) -> Text<'a> {
    txt(s, tone).size(fonts::SMALL).line_height(LineHeight::Absolute(Pixels(fonts::SMALL_LINE)))
}

/// Body text that wraps at words, each wrapped line 16 px.
pub fn wrapped<'a>(s: impl text::IntoFragment<'a>, tone: Tone) -> Text<'a> {
    txt(s, tone).wrapping(Wrapping::WordOrGlyph)
}

pub fn span<'a>(s: &Span) -> Text<'a> {
    let t = txt(s.text.clone(), s.tone);
    if s.bold { t.font(fonts::MONO_BOLD) } else { t }
}

/// Spans side by side, as one line.
pub fn line<'a, M: 'a>(spans: &[Span]) -> Row<'a, M> {
    Row::with_children(spans.iter().map(|s| span(s).into())).height(Length::Fixed(fonts::LINE))
}

/// A widget the tests and the simulator find by its id, which takes a click when it has a
/// message for it.
pub fn clickable<'a, M: Clone + 'a>(
    id: String,
    content: impl Into<Element<'a, M>>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let inner: Element<'a, M> = match on_press {
        Some(m) => mouse_area(content).on_press(m).interaction(mouse::Interaction::Pointer).into(),
        None => content.into(),
    };
    container(inner).id(id).into()
}

/// A button drawn as its words in a tone, under its id (5.7.6).
pub fn words_button<'a, M: Clone + 'a>(id: String, words: String, tone: Tone, on_press: M) -> Element<'a, M> {
    let b = button(label(words)).padding(0).style(theme::words_button(tone)).on_press(on_press);
    container(b).id(id).into()
}

/// A button boxed in its tone, under its id: the input's offers and a hand's (fac-66).
pub fn boxed_button<'a, M: Clone + 'a>(id: String, words: String, tone: Tone, on_press: M) -> Element<'a, M> {
    let b = button(label(words)).padding([1.0, 8.0]).style(theme::boxed_button(tone)).on_press(on_press);
    container(b).id(id).into()
}

/// A section's heading with its `send to orchestrator` at the right (fac-249): the button's id is
/// `review:<section>:<target>`.
pub fn sectioned<'a, M: Clone + 'a>(heading: Vec<Element<'a, M>>, id: String, on_press: M) -> Element<'a, M> {
    let send = boxed_button(id, "send to orchestrator".into(), Tone::Green, on_press);
    let line = row(heading).push(iced::widget::space().width(Length::Fill)).push(send);
    line.align_y(iced::alignment::Vertical::Center).into()
}

/// A thumbnail's height: three lines (fac-31).
pub const THUMB: f32 = 3.0 * fonts::LINE;

/// An image file as a thumbnail `THUMB` px high, in its own proportions. The renderer reads the
/// file; one it can't read draws nothing.
pub fn thumbnail<'a, M: 'a>(path: &str) -> Element<'a, M> {
    let handle = iced::widget::image::Handle::from_path(path);
    iced::widget::image(handle).height(Length::Fixed(THUMB)).into()
}

pub fn gap<'a, M: 'a>(width: f32) -> Element<'a, M> {
    iced::widget::space().width(Length::Fixed(width)).into()
}

/// A row of widgets, centred on one 16 px line.
pub fn centred<'a, M: 'a>(children: Vec<Element<'a, M>>) -> Row<'a, M> {
    row(children).height(Length::Fixed(fonts::LINE)).align_y(iced::alignment::Vertical::Center)
}

/// How many characters of the mono font fit in a width.
pub fn chars_in(width: f32) -> usize {
    (width / fonts::ADVANCE).floor().max(0.0) as usize
}

/// A text cut to `n` characters, ending in `…` when cut.
pub fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A text cut to `n` characters in its middle, `…` between its start and its end.
pub fn cut_middle(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_owned();
    }
    let end = n.saturating_sub(1).div_ceil(2);
    let start: String = chars[..n.saturating_sub(1) - end].iter().collect();
    let end: String = chars[chars.len() - end..].iter().collect();
    format!("{start}…{end}")
}

/// A 1 px line across its width, in a style.
pub fn hline<'a, M: 'a>(style: fn(&iced::Theme) -> iced::widget::container::Style) -> Element<'a, M> {
    container(text("")).width(Length::Fill).height(Length::Fixed(1.0)).style(style).into()
}

pub fn gap_v<'a, M: 'a>(height: f32) -> Element<'a, M> {
    iced::widget::space().height(Length::Fixed(height)).into()
}

/// A vertical scroll area with the thin scrollbar.
pub fn scroll<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> iced::widget::Scrollable<'a, M> {
    scroll_with(content, iced::widget::scrollable::Scrollbar::new().width(2).scroller_width(2).margin(0))
}

/// A vertical scroll area with this scrollbar, in the scrollbars' style.
pub fn scroll_with<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    bar: iced::widget::scrollable::Scrollbar,
) -> iced::widget::Scrollable<'a, M> {
    iced::widget::scrollable(content)
        .direction(iced::widget::scrollable::Direction::Vertical(bar))
        .style(theme::scrollbar)
        .height(Length::Fill)
}
