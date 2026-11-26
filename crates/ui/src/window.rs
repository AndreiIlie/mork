//! The window's frame (5.1, 5.4): panels edge to edge with one divider between neighbours
//! (fac-103), under the wrapper that holds the window's keys and its input's focus.

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Padding, Task};

use crate::app::{App, Message, WindowOp};
use crate::spans::{self, Tone};
use crate::widgets::{self, boxed_button, clickable, words_button};
use crate::{fonts, place, rooms, side, theme, wrappers};

/// The target size: a maximized window on the operator's 1080p monitor (5).
pub const TARGET: (f32, f32) = (1853.0, 1048.0);
/// The rooms panel's width, and the side column's, the dividers beside them left out (5.4).
pub const ROOMS_W: f32 = 284.0;
pub const SIDE_W: f32 = 366.0;
/// A side panel's padding, and the place's on its left and right.
pub const PAD: f32 = 10.0;
pub const INSET: f32 = 20.0;
/// The side column's width inside its padding.
pub const SIDE_INNER: f32 = SIDE_W - 2.0 * PAD;
/// The top bar's height, the divider under it included.
pub const TOP_H: f32 = 28.0;

/// The id of the place's conversation, which its wrapper scrolls.
pub const CONVERSATION: &str = "conversation";

/// The app's window settings (5.1.1): maximized, with no decorations, at the position the window
/// last had, which puts it on the monitor it was last on (5.1.1, M4.16).
pub fn settings_at(position: Option<(i32, i32)>) -> iced::window::Settings {
    let position = position.map_or(iced::window::Position::Default, |(x, y)| {
        // why: a desktop's coordinates are far inside f32's exact integers.
        #[allow(clippy::cast_precision_loss)]
        iced::window::Position::Specific(iced::Point::new(x as f32, y as f32))
    });
    iced::window::Settings {
        position,
        size: iced::Size::new(TARGET.0, TARGET.1),
        maximized: true,
        decorations: false,
        // A close request (Alt+F4, the dock) is the app's to carry out: the engine shuts down
        // cleanly first (2.7, 5.1.4).
        exit_on_close_request: false,
        // The desktop file's name, so the dock shows the launcher's icon on the window.
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "mork".to_owned(),
            ..Default::default()
        },
        ..iced::window::Settings::default()
    }
}

/// The whole window: the top bar over the panels, on the backdrop.
pub fn frame(app: &App) -> Element<'_, Message> {
    // The rooms pad themselves left and right, so a row's band spans the panel.
    let rooms = Padding::ZERO.top(PAD).bottom(PAD);
    let body = row![
        panel(rooms::view(app), Length::Fixed(ROOMS_W), rooms),
        divider(Length::Fixed(theme::BORDER), Length::Fill),
        panel(place::view(app), Length::Fill, Padding::ZERO),
    ]
    .push(right(app))
    .height(Length::Fill);
    let content = column![top(app), divider(Length::Fill, Length::Fixed(theme::BORDER)), body];
    let window = container(content).width(Length::Fill).height(Length::Fill).style(theme::backdrop);
    wrappers::root(window, place::INPUT)
}

// A panel has no fill nor edge of its own: the backdrop's black shows, its neighbours' dividers
// tell it apart.
fn panel<'a>(content: Element<'a, Message>, width: Length, padding: Padding) -> container::Container<'a, Message> {
    container(content).padding(padding).width(width).height(Length::Fill).clip(true)
}

fn divider<'a>(width: Length, height: Length) -> Element<'a, Message> {
    container(text("")).width(width).height(height).style(theme::divider).into()
}

fn right(app: &App) -> Option<Element<'_, Message>> {
    let col = side::view(app)?;
    Some(row![divider(Length::Fixed(theme::BORDER), Length::Fill), col].into())
}

fn top(app: &App) -> Element<'_, Message> {
    let controls = row![
        words_button("tunnels".into(), "tunnels".into(), Tone::Dim, Message::ShowTunnels),
        words_button("ctl:min".into(), "–".into(), Tone::Dim, Message::Window(WindowOp::Minimize)),
        words_button("ctl:max".into(), "□".into(), Tone::Dim, Message::Window(WindowOp::ToggleMaximize)),
        words_button("ctl:close".into(), "×".into(), Tone::Dim, Message::Window(WindowOp::Close)),
    ]
    .spacing(fonts::ADVANCE * 2.0);
    let t = &app.screen.top;
    let mut line = row![widgets::line(&spans::top_head(t))].align_y(iced::alignment::Vertical::Center);
    for (i, u) in spans::top_updates(t).into_iter().enumerate() {
        let tone = u.words.last().map_or(Tone::Green, |w| w.tone);
        line = line.push(widgets::line(&u.words));
        if let Some((label, command)) = u.button {
            let press = Message::TopButton { label: label.clone(), command: Box::new(command) };
            line = line.push(widgets::gap(fonts::ADVANCE)).push(boxed_button(format!("top:{i}"), label, tone, press));
        }
    }
    let line =
        line.push(widgets::line(&spans::top_tail(t))).push(iced::widget::space().width(Length::Fill)).push(controls);
    let bar = container(line)
        .padding(Padding::ZERO.horizontal(14.0))
        .width(Length::Fill)
        .height(Length::Fixed(TOP_H - 1.0))
        .align_y(iced::alignment::Vertical::Center);
    clickable("top".into(), bar, Some(Message::TopPress))
}

/// The task for one of the window's own controls.
pub fn window_task<M: Send + 'static>(op: WindowOp) -> Task<M> {
    iced::window::latest().then(move |id| match id {
        Some(id) => match op {
            WindowOp::Drag => iced::window::drag(id),
            WindowOp::ToggleMaximize => iced::window::toggle_maximize(id),
            WindowOp::Minimize => iced::window::minimize(id, true),
            WindowOp::Close => iced::window::close(id),
        },
        None => Task::none(),
    })
}

/// The input's focus at the start; the root wrapper keeps it there after (`ui::wrappers`).
pub fn focus_input<M: Send + 'static>() -> Task<M> {
    iced::widget::operation::focus(place::INPUT)
}
