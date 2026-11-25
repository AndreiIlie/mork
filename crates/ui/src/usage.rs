use factory_types::view::Share;
use iced::Element;
use iced::widget::Column;

use crate::app::{App, Message};
use crate::spans::{self, Span, Tone};
use crate::widgets;

/// The usage pools and their window rows, aligned across the section.
pub(crate) fn body(app: &App) -> Element<'_, Message> {
    // The engine's order holds: the live Claude account before the signed-out one.
    let mut pools: Vec<(&str, Vec<_>)> = Vec::new();
    let mut widths = [0usize; 3];
    for s in &app.screen.top.usage {
        let cells = spans::usage_cells(s, app.now_ms);
        widths[0] = widths[0].max(s.window.chars().count());
        for (width, cell) in widths[1..].iter_mut().zip(&cells) {
            *width = (*width).max(cell.text.chars().count());
        }
        match pools.last_mut() {
            Some((name, rows)) if *name == s.cli => rows.push((s, cells)),
            _ => pools.push((s.cli.as_str(), vec![(s, cells)])),
        }
    }
    let mut col = Column::new();
    for (name, rows) in pools {
        let age = rows.iter().find_map(|(s, _)| s.read_ago.as_ref());
        let label = age.map_or_else(|| name.to_owned(), |age| format!("{name} · read {age} ago"));
        let tone = if rows.iter().all(|(s, _)| s.signed_out) { Tone::Dim } else { Tone::Text };
        let account = rows.iter().find_map(|(s, _)| s.switch_account.clone());
        col = col.push(widgets::clickable(
            format!("usage:{name}"),
            widgets::txt(label, tone),
            account.map(Message::ClaudeAccount),
        ));
        for (s, cells) in rows {
            col = col.push(window(s, cells, widths));
        }
    }
    col.into()
}

fn window(s: &Share, cells: [Span; 3], widths: [usize; 3]) -> Element<'_, Message> {
    let mut line = vec![Span::new(format!("  {:<width$}  ", s.window, width = widths[0]), Tone::Dim)];
    for (i, mut cell) in cells.into_iter().enumerate() {
        if i < 2 {
            cell.text = format!("{:<width$}  ", cell.text, width = widths[i + 1]);
        }
        line.push(cell);
    }
    widgets::line(&line).into()
}
