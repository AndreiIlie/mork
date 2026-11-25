//! The window owns these controls (fac-35), so opening a forward never sends an agent command.

use factory_types::web_tunnel::{Environment, Status};
use iced::widget::{Column, column, row};
use iced::{Element, Length};

use crate::app::{App, Effect, Message};
use crate::spans::Tone;
use crate::widgets::{self, bold, txt, words_button};

impl App {
    pub(crate) fn tunnel(&mut self, env: Environment, open: bool) -> Vec<Effect> {
        let status = &mut self.tunnels[env.index()];
        let permitted = match status {
            Status::Closed | Status::Failed(_) | Status::Dropped(_) => open,
            Status::Opening | Status::Open => !open,
            Status::Closing => false,
        };
        if !permitted {
            return Vec::new();
        }
        *status = if open { Status::Opening } else { Status::Closing };
        vec![Effect::Tunnel(env, open)]
    }

    pub(crate) fn tunnel_page(&self, env: Environment) -> Vec<Effect> {
        if self.tunnels[env.index()] == Status::Open { vec![Effect::Link(env.url().into())] } else { Vec::new() }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let rows = Environment::ALL.into_iter().map(|env| environment(env, &app.tunnels[env.index()]));
    column![
        Column::with_children(rows).spacing(16),
        txt("Tunnels close when Factory exits and open again when it starts.", Tone::Dim),
    ]
    .spacing(16)
    .into()
}

fn environment(env: Environment, status: &Status) -> Element<'_, Message> {
    let (label, tone) = match status {
        Status::Closed => ("closed", Tone::Dim),
        Status::Opening => ("opening…", Tone::Orange),
        Status::Open => ("open", Tone::Green),
        Status::Closing => ("closing…", Tone::Dim),
        Status::Failed(_) => ("failed", Tone::Orange),
        Status::Dropped(_) => ("dropped", Tone::Orange),
    };
    let name = env.name();
    let mut line = row![bold(name, Tone::Bright), txt(label, tone)].spacing(12);
    let action = match status {
        Status::Closed | Status::Failed(_) | Status::Dropped(_) => Some(("open", true)),
        Status::Opening | Status::Open => Some(("close", false)),
        Status::Closing => None,
    };
    if let Some((label, open)) = action {
        line = line.push(words_button(
            format!("tunnel:{name}:{label}"),
            label.into(),
            Tone::Green,
            Message::Tunnel(env, open),
        ));
    }
    let address = format!("localhost:{}/root", env.port());
    let link = widgets::clickable(
        format!("tunnel:{name}:link"),
        txt(address, if *status == Status::Open { Tone::Green } else { Tone::Dim }),
        (*status == Status::Open).then_some(Message::TunnelPage(env)),
    );
    let mut col = column![line, link].spacing(4);
    if let Status::Failed(why) | Status::Dropped(why) = status {
        col = col.push(widgets::wrapped(crate::text::clean(why), Tone::Orange).width(Length::Fill));
    }
    col.into()
}
