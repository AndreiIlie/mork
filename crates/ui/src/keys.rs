//! The input's keys (5.6.3, 5.7.7, 5.8): the window's own keys become messages, everything
//! else edits the draft. The paste key is the window's, so an image pastes too (fac-31).

use iced::keyboard::key::Named;
use iced::keyboard::{self, Modifiers};
use iced::widget::text_editor::{Binding, KeyPress, Status};

use crate::app::{Key, Message};

/// The input's key bindings. `multi_line`: the draft has more than one line, so ↑ and ↓ move
/// the cursor in it rather than scroll the conversation.
pub fn binding(multi_line: bool) -> impl Fn(KeyPress) -> Option<Binding<Message>> {
    move |press: KeyPress| {
        if !matches!(press.status, Status::Focused { .. }) {
            return None;
        }
        if let Some(k) = window_key(&press.key, press.modifiers, multi_line) {
            return Some(Binding::Custom(Message::Key(k)));
        }
        if press.key == keyboard::Key::Named(Named::Enter) {
            return Some(Binding::Enter);
        }
        match Binding::from_key_press(press) {
            Some(Binding::Paste) => Some(Binding::Custom(Message::Paste)),
            other => other,
        }
    }
}

/// The window's key for a key press, if it is one (5.6.3, 5.7, 5.8).
pub fn window_key(key: &keyboard::Key, m: Modifiers, multi_line: bool) -> Option<Key> {
    let keyboard::Key::Named(named) = key else {
        return alt_digit(key, m);
    };
    let plain = !m.shift() && !m.control() && !m.alt() && !m.logo();
    match named {
        Named::Enter if plain => Some(Key::Enter),
        Named::Tab if plain => Some(Key::Tab),
        Named::Escape => Some(Key::Esc),
        Named::ArrowUp if m.control() => Some(Key::CtrlUp),
        Named::ArrowDown if m.control() => Some(Key::CtrlDown),
        Named::ArrowLeft if m.control() => Some(Key::CtrlLeft),
        Named::ArrowRight if m.control() => Some(Key::CtrlRight),
        Named::ArrowUp if plain && !multi_line => Some(Key::Up),
        Named::ArrowDown if plain && !multi_line => Some(Key::Down),
        _ => None,
    }
}

fn alt_digit(key: &keyboard::Key, m: Modifiers) -> Option<Key> {
    let keyboard::Key::Character(c) = key else { return None };
    if !m.alt() || m.control() {
        return None;
    }
    match c.as_str().parse::<u8>() {
        Ok(n @ 1..=9) => Some(Key::Alt(n)),
        _ => None,
    }
}
