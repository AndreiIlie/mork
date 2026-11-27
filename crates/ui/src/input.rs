//! Input helpers for the native editor and its command buttons.

use mork_types::Command;
pub use mork_types::input::{Context, Parsed, draft, names_a_place, parse, shown_nowhere};

/// An input holding a draft, its cursor at the end.
pub(crate) fn draft_content(text: &str) -> iced::widget::text_editor::Content {
    use iced::widget::text_editor::{Action, Content, Motion};
    let mut c = Content::with_text(text);
    c.perform(Action::Move(Motion::DocumentEnd));
    c
}

/// A button's command, read as a typed one is (5.10.4): `usage` shows in the place's area, any
/// other is sent, the engine validating it.
pub fn pressed(command: &Command) -> Parsed {
    match command {
        Command::Usage {} => Parsed::Usage(Command::Usage {}),
        c => Parsed::Send(c.clone()),
    }
}
