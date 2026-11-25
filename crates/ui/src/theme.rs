//! The palette (5.2) and the frame's named styles (fac-66, fac-103's Graphite): defined once,
//! here. No other source file holds a colour.

use iced::widget::{button, container, scrollable, text_editor};
use iced::{Background, Border, Color, Shadow, Theme};

use crate::spans::Tone;

const fn rgb(hex: u32) -> Color {
    let [_, r, g, b] = hex.to_be_bytes();
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

const fn alpha(c: Color, a: f32) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a }
}

/// One flat black for the backdrop, every panel, the place and the input's field.
pub const BACKGROUND: Color = rgb(0x000a_0a0a);
/// The 1 px edge that alone tells surfaces apart: the dividers between panels, a card's edge.
pub const EDGE: Color = rgb(0x004e_4e4e);
/// Names that matter, the selected room, Andrei's words, the cursor.
pub const BRIGHT: Color = rgb(0x00ec_ecec);
/// The foreman's name: a grey a step brighter than body.
pub const STRONG: Color = rgb(0x00d6_d6d6);
/// Body text.
pub const TEXT: Color = rgb(0x00b8_b8b8);
/// Meta, times, engine markers, hints, the grey light.
pub const DIM: Color = rgb(0x007a_7a7a);
/// Rules, idle squares, stages still to come, a card with no light.
pub const RULE: Color = rgb(0x002d_2d2d);
/// Working, the current stage, the green light, buttons.
pub const GREEN: Color = rgb(0x0056_c37f);
/// The yellow light, yellow asks, counts, warnings.
pub const ORANGE: Color = rgb(0x00e6_944a);
/// The red light, red asks, limits reached.
pub const RED: Color = rgb(0x00d9_5f50);
/// The open or picked mark: the open room's door, the input's `›`, a picked card's edge.
pub const WHITE: Color = rgb(0x00f5_f5f5);
/// A lit specification mark's edge, and an idea's `spec?`.
pub const LIT: Color = rgb(0x00bd_bdbd);

// The conversation's fills (fac-39), opaque on the black, so its words keep their contrast
// whatever the blending.

/// Cursor's selected row: Andrei's bubble, and the open room's band.
pub const SELECTED: Color = rgb(0x001f_1f1f);
/// Andrei's bubble.
pub const BUBBLE: Color = SELECTED;
/// A divider's line, inline code's fill.
pub const SEAM: Color = rgb(0x002d_2d2d);
/// A table's rules.
pub const TABLE_RULE: Color = rgb(0x0024_2424);
/// A fenced block's fill, and its edge.
pub const FENCE: Color = rgb(0x0011_1111);
pub const FENCE_EDGE: Color = rgb(0x002d_2d2d);
/// The fill of a message to Andrei.
pub const PANEL: Color = rgb(0x0016_1616);
/// An open ask's fill.
pub const ASK_FILL: Color = rgb(0x0015_110d);

/// An edge's width, in px: a divider's, a card's. No corner is rounded.
pub const BORDER: f32 = 1.0;

/// The Iced theme built on the palette.
pub fn iced_theme() -> Theme {
    Theme::custom(
        "factory".to_owned(),
        iced::theme::Palette {
            background: BACKGROUND,
            text: TEXT,
            primary: GREEN,
            success: GREEN,
            warning: ORANGE,
            danger: RED,
        },
    )
}

/// A tone's colour (5.2).
pub fn color(tone: Tone) -> Color {
    match tone {
        Tone::Bright => BRIGHT,
        Tone::Strong => STRONG,
        Tone::Text => TEXT,
        Tone::Dim => DIM,
        Tone::Rule => RULE,
        Tone::Edge => EDGE,
        Tone::Green => GREEN,
        Tone::Orange => ORANGE,
        Tone::Red => RED,
        Tone::White => WHITE,
    }
}

fn fill(c: Color) -> container::Style {
    container::Style { background: Some(c.into()), ..container::Style::default() }
}

fn edged(c: Color) -> Border {
    Border { color: c, width: BORDER, ..Border::default() }
}

/// The window's backdrop: the one flat black, which every panel shows.
pub fn backdrop(_: &Theme) -> container::Style {
    fill(BACKGROUND)
}

/// A divider: the line neighbouring panels share, and the one under the top bar.
pub fn divider(_: &Theme) -> container::Style {
    fill(EDGE)
}

/// The room header's underline: a rule.
pub fn underline(_: &Theme) -> container::Style {
    fill(RULE)
}

/// The current stage: a faint green fill.
pub fn stage_pill(_: &Theme) -> container::Style {
    fill(alpha(GREEN, 0.10))
}

/// The open room's band: Cursor's selected row.
pub fn room_open(_: &Theme) -> container::Style {
    fill(SELECTED)
}

/// A diff's added line, across the column: a faint green (fac-105).
pub fn added_row(_: &Theme) -> container::Style {
    fill(alpha(GREEN, 0.12))
}

/// A diff's removed line, across the column: a faint red.
pub fn removed_row(_: &Theme) -> container::Style {
    fill(alpha(RED, 0.12))
}

/// The room picked with the keys: half the open room's band.
pub fn room_picked(_: &Theme) -> container::Style {
    fill(alpha(SELECTED, 0.5))
}

/// A card edged in a tone: the surfaces' grey, or white while it is picked. A hand's colour is
/// never its edge: Andrei found a status-coloured edge too heavy (fac-103).
pub fn card(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { border: edged(color(tone)), ..container::Style::default() }
}

/// A mark: a box edged faintly in its tone, or in the lit grey while lit.
pub fn mark(tone: Tone, lit: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let edge = if lit { LIT } else { alpha(color(tone), 0.4) };
        container::Style { border: edged(edge), ..container::Style::default() }
    }
}

/// A cut title shown whole under the pointer (fac-84): the black, opaque over the rows beneath,
/// edged.
pub fn tooltip(_: &Theme) -> container::Style {
    container::Style { border: edged(EDGE), ..fill(BACKGROUND) }
}

/// A button drawn as its words alone, in a tone: no background, no border.
pub fn words_button(tone: Tone) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let text_color = match status {
            button::Status::Hovered | button::Status::Pressed => BRIGHT,
            button::Status::Active | button::Status::Disabled => color(tone),
        };
        button::Style { text_color, ..button::Style::default() }
    }
}

/// A button boxed in its tone: a faint fill and edge, its words and edge brighter on hover.
pub fn boxed_button(tone: Tone) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let c = color(tone);
        let (text_color, edge) = match status {
            button::Status::Hovered | button::Status::Pressed => (BRIGHT, c),
            button::Status::Active | button::Status::Disabled => (c, alpha(c, 0.15)),
        };
        button::Style {
            background: Some(alpha(c, 0.015).into()),
            text_color,
            border: edged(edge),
            ..button::Style::default()
        }
    }
}

/// The scrollbars: a thin rule-coloured scroller on no rail.
pub fn scrollbar(_: &Theme, _: scrollable::Status) -> scrollable::Style {
    use scrollable::{AutoScroll, Rail, Scroller, Style};
    let rail = Rail {
        background: None,
        border: Border::default(),
        scroller: Scroller { background: RULE.into(), border: Border::default() },
    };
    Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: AutoScroll {
            background: BACKGROUND.into(),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: DIM,
        },
    }
}

/// Andrei's words: a bubble in the selected row's grey.
pub fn bubble(_: &Theme) -> container::Style {
    fill(BUBBLE)
}

/// An open ask: its warm fill, edged like every surface: a hand's colour is never its edge.
pub fn ask_open(_: &Theme) -> container::Style {
    container::Style { border: edged(EDGE), ..fill(ASK_FILL) }
}

/// A message to Andrei: a dark-grey panel, edged thinly.
pub fn panel(_: &Theme) -> container::Style {
    container::Style { border: edged(EDGE), ..fill(PANEL) }
}

/// A card's switch's track (fac-258), rounded as a switch is: faint green while on, edged grey
/// off, faded while the engine would refuse its flip.
pub fn switch_track(on: bool, live: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let (fill, edge) = if on { (alpha(GREEN, 0.25), GREEN) } else { (BACKGROUND, EDGE) };
        let fade = if live { 1.0 } else { 0.4 };
        let border = Border { color: alpha(edge, fade), width: BORDER, radius: SWITCH_RADIUS.into() };
        container::Style { background: Some(alpha(fill, fill.a * fade).into()), border, ..container::Style::default() }
    }
}

/// A switch's knob: green while on, dim off, faded as its track is.
pub fn switch_knob(on: bool, live: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let c = alpha(if on { GREEN } else { DIM }, if live { 1.0 } else { 0.4 });
        let border = Border { radius: SWITCH_RADIUS.into(), ..Border::default() };
        container::Style { background: Some(c.into()), border, ..container::Style::default() }
    }
}

const SWITCH_RADIUS: f32 = 7.0;

/// A hand's dot in its colour.
pub fn dot(tone: Tone) -> impl Fn(&Theme) -> container::Style {
    move |_| fill(color(tone))
}

/// A divider's line.
pub fn seam(_: &Theme) -> container::Style {
    fill(SEAM)
}

/// A table's rules.
pub fn table_rule(_: &Theme) -> container::Style {
    fill(TABLE_RULE)
}

/// A fenced block: a darker well, edged.
pub fn fence(_: &Theme) -> container::Style {
    container::Style { border: edged(FENCE_EDGE), ..fill(FENCE) }
}

/// Inline code's highlight: the seam's fill.
pub fn code_highlight() -> (Background, Border) {
    (SEAM.into(), Border::default())
}

/// The input's field: the black, edged like every surface.
pub fn field(_: &Theme) -> container::Style {
    container::Style { border: edged(EDGE), ..fill(BACKGROUND) }
}

/// The input inside its field: Andrei's words bright, a dim placeholder, a bright cursor.
pub fn input(_: &Theme, _: text_editor::Status) -> text_editor::Style {
    text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        placeholder: DIM,
        value: BRIGHT,
        selection: RULE,
    }
}
