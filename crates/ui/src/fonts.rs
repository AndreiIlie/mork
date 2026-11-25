//! DejaVu Sans Mono, and Lucide's icon font for the icons (fac-103), bundled so every render is
//! the same (5.2).

use iced::Font;

pub const REGULAR_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSansMono.ttf");
pub const BOLD_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSansMono-Bold.ttf");
/// Markdown's italic (fac-39).
pub const ITALIC_BYTES: &[u8] = include_bytes!("../fonts/DejaVuSansMono-Oblique.ttf");
/// Lucide 1.59.0's font, as lucide-static ships it; ISC and MIT, in `fonts/LICENSE-Lucide`.
pub const ICONS_BYTES: &[u8] = include_bytes!("../fonts/Lucide.ttf");

/// The font for everything.
pub const MONO: Font = Font::with_name("DejaVu Sans Mono");
pub const MONO_BOLD: Font = Font { weight: iced::font::Weight::Bold, ..MONO };
pub const MONO_ITALIC: Font = Font { style: iced::font::Style::Italic, ..MONO };
pub const ICONS: Font = Font::with_name("lucide");

/// Body text: 13 px on 16 px lines.
pub const SIZE: f32 = 13.0;
pub const LINE: f32 = 16.0;
/// Footers and hints: 11 px on 14 px lines.
pub const SMALL: f32 = 11.0;
pub const SMALL_LINE: f32 = 14.0;
/// A character's advance in the mono font at 13 px: DejaVu Sans Mono's 1233 units of 2048.
pub const ADVANCE: f32 = SIZE * 1233.0 / 2048.0;
