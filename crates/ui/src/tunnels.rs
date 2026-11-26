//! The window owns these controls (fac-35), so opening a forward never sends an agent command.

use iced::widget::{Column, column, row};
use iced::{Element, Length};
use mork_types::config::TunnelConfig;
use mork_types::web_tunnel::Status;

use crate::app::{App, Effect, Message};
use crate::spans::Tone;
use crate::widgets::{self, bold, txt, words_button};

impl App {
    pub(crate) fn tunnel(&mut self, name: String, open: bool) -> Vec<Effect> {
        if !self.settings.tunnels.contains_key(&name) {
            return Vec::new();
        }
        let status = self.tunnels.entry(name.clone()).or_default();
        let permitted = match status {
            Status::Closed | Status::Failed(_) | Status::Dropped(_) => open,
            Status::Opening | Status::Open => !open,
            Status::Closing => false,
        };
        if !permitted {
            return Vec::new();
        }
        *status = if open { Status::Opening } else { Status::Closing };
        vec![Effect::Tunnel(name, open)]
    }

    pub(crate) fn tunnel_page(&self, name: &str) -> Vec<Effect> {
        if self.tunnels.get(name) == Some(&Status::Open)
            && let Some(cfg) = self.settings.tunnels.get(name)
        {
            vec![Effect::Link(cfg.url.clone())]
        } else {
            Vec::new()
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    static CLOSED: Status = Status::Closed;
    let rows =
        app.settings.tunnels.iter().map(|(name, cfg)| tunnel_row(name, cfg, app.tunnels.get(name).unwrap_or(&CLOSED)));
    column![
        Column::with_children(rows).spacing(16),
        txt("Tunnels close when Mork exits and open again when it starts.", Tone::Dim),
    ]
    .spacing(16)
    .into()
}

fn tunnel_row<'a>(name: &'a str, cfg: &'a TunnelConfig, status: &'a Status) -> Element<'a, Message> {
    let (label, tone) = match status {
        Status::Closed => ("closed", Tone::Dim),
        Status::Opening => ("opening…", Tone::Orange),
        Status::Open => ("open", Tone::Green),
        Status::Closing => ("closing…", Tone::Dim),
        Status::Failed(_) => ("failed", Tone::Orange),
        Status::Dropped(_) => ("dropped", Tone::Orange),
    };
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
            Message::Tunnel(name.to_owned(), open),
        ));
    }
    let link = widgets::clickable(
        format!("tunnel:{name}:link"),
        txt(cfg.url.clone(), if *status == Status::Open { Tone::Green } else { Tone::Dim }),
        (*status == Status::Open).then_some(Message::TunnelPage(name.to_owned())),
    );
    let mut col = column![line, link].spacing(4);
    if let Status::Failed(why) | Status::Dropped(why) = status {
        col = col.push(widgets::wrapped(crate::text::clean(why), Tone::Orange).width(Length::Fill));
    }
    col.into()
}
