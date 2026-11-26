//! What the window does with a key, a line, a press or a click (section 5): the handlers
//! behind `App::update`.

use mork_types::command::Delivery;
use mork_types::config::Effort;
use mork_types::orchestrator::ReviewSection;
use mork_types::reply::Role;
use mork_types::view::{ModelChoice, PlaceKind};
use mork_types::{CardId, Command};

use crate::app::{App, DOUBLE_CLICK_MS, Effect, Key, Overlay, SCROLL_STEP, ScrollOp, WindowOp};
use crate::input::{self, Context, Parsed, draft_content};
use crate::session::Tab;

impl App {
    pub(crate) fn key(&mut self, k: Key) -> Vec<Effect> {
        match k {
            Key::Tab => {
                self.fold_all();
                Vec::new()
            }
            Key::Esc => self.escape(),
            Key::Enter => self.enter(),
            Key::CtrlUp => self.select(-1),
            Key::CtrlDown => self.select(1),
            // The picked row's card folds or unfolds; else main's tab beside.
            Key::CtrlLeft | Key::CtrlRight => {
                if !self.fold_picked(k == Key::CtrlLeft) && self.session.place == "main" {
                    self.session.tab = self.session.tab.beside(if k == Key::CtrlLeft { -1 } else { 1 });
                }
                Vec::new()
            }
            Key::Alt(n) => self.alt(n),
            Key::Up => self.arrow(-1),
            Key::Down => self.arrow(1),
        }
    }

    pub(crate) fn escape(&mut self) -> Vec<Effect> {
        if self.overlay.take().is_some() {
            return Vec::new();
        }
        self.selected = None;
        if self.session.place == "main" { Vec::new() } else { self.open("main".into()) }
    }

    pub(crate) fn enter(&mut self) -> Vec<Effect> {
        let line = self.input.text();
        if self.takes_input() && (!line.trim().is_empty() || !self.draft_images().is_empty()) {
            return self.submit(&line, false);
        }
        if let Some(place) = self.selected.take() {
            return self.open(place);
        }
        match self.board_row {
            Some(i) if self.on_board() => match crate::board::rows(&self.screen, &self.board_open).get(i) {
                Some((_, id)) => {
                    let id = id.to_string();
                    self.board(&id)
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Send now (fac-78): the draft sent as Enter sends it, its message cutting the recipient's
    /// running turn. An empty draft sends nothing; a command is no message, and stays in the draft.
    pub(crate) fn send_now(&mut self) -> Vec<Effect> {
        if !self.takes_input() {
            return Vec::new();
        }
        let line = self.input.text();
        if line.trim_start().starts_with('/') {
            self.note = Some("✗ send now sends a message, not a command: nothing sent".into());
            return Vec::new();
        }
        if line.trim().is_empty() && self.draft_images().is_empty() {
            return Vec::new();
        }
        self.submit(&line, true)
    }

    /// The line typed, and the images pasted with it, read and acted on once (5.10). The images
    /// leave the draft only with the `say` that carries them.
    pub(crate) fn submit(&mut self, line: &str, now: bool) -> Vec<Effect> {
        let images = self.draft_images().to_vec();
        let mut parsed = input::draft(line, &images, &self.context());
        if now && let Parsed::Send(Command::Say { delivery, .. }) = &mut parsed {
            *delivery = Some(Delivery::Now);
        }
        if matches!(&parsed, Parsed::Send(Command::Say { images: Some(carried), .. }) if *carried == images) {
            self.session.draft_images.remove(&self.session.place);
        }
        self.set_input("");
        self.note = None;
        self.act_on(parsed)
    }

    /// Whether the place shown has an input: main has none, on any tab (fac-230), so nothing
    /// typed, pasted or kept as a draft is sent from it.
    pub fn takes_input(&self) -> bool {
        self.session.place != "main"
    }

    pub fn draft_images(&self) -> &[String] {
        self.session.draft_images.get(&self.session.place).map_or(&[], Vec::as_slice)
    }

    /// The clipboard's image, saved, joins the draft of the place it was pasted in, shown or not:
    /// the read may end after a switch. Why there is none is said above the input (fac-31).
    pub(crate) fn pasted(&mut self, place: String, image: Result<String, String>) {
        if place == "main" {
            return;
        }
        match image {
            Ok(path) => {
                self.note = None;
                self.session.draft_images.entry(place).or_default().push(path);
            }
            Err(e) => self.note = Some(format!("✗ {}", crate::text::clean(&e))),
        }
    }

    /// A pasted image's ×: it leaves the draft.
    pub(crate) fn unpaste(&mut self, i: usize) {
        let place = self.session.place.clone();
        let Some(images) = self.session.draft_images.get_mut(&place) else { return };
        if i < images.len() {
            images.remove(i);
        }
        if images.is_empty() {
            self.session.draft_images.remove(&place);
        }
    }

    // Typed and pressed commands take this one path (5.7.6, 5.10.4).
    pub(crate) fn act_on(&mut self, parsed: Parsed) -> Vec<Effect> {
        match parsed {
            Parsed::Send(c) => vec![Effect::Send(c)],
            Parsed::Open(place) => self.open(place),
            Parsed::Help => {
                self.overlay = Some(Overlay::Help);
                Vec::new()
            }
            Parsed::Usage(c) => {
                self.overlay = Some(Overlay::Usage { answer: None });
                vec![Effect::Send(c)]
            }
            Parsed::Note(n) => {
                self.note = Some(n);
                Vec::new()
            }
            Parsed::Nothing => Vec::new(),
        }
    }

    /// Where a line is typed: the place, its card, and who its plain text goes to.
    pub fn context(&self) -> Context {
        let place = self.session.place.clone();
        let view = self.place();
        let kind =
            view.map(|v| &v.kind).or_else(|| self.screen.rooms.iter().find(|r| r.place == place).map(|r| &r.kind));
        let card = match kind {
            Some(
                PlaceKind::Card { card }
                | PlaceKind::Spec { card }
                | PlaceKind::Challenge { card }
                | PlaceKind::Dm { card, .. }
                | PlaceKind::Child { card, .. },
            ) => Some(card.to_string()),
            Some(PlaceKind::Main | PlaceKind::Other) => None,
            None if input::names_a_place(&place) && place != "main" => Some(place.clone()),
            None => None,
        };
        let asker = self.replying_to.as_deref().and_then(|ask| {
            self.screen.hands.iter().find(|h| h.ask.as_str() == ask && h.place.as_str() == place).and_then(|h| h.role)
        });
        // An answer goes to the seat that asked, in that seat's own room (fac-558, fac-633).
        let to = match asker {
            Some(r @ (Role::Planner | Role::Foreman)) => r.as_str().into(),
            _ => view.map_or_else(String::new, |v| v.input_to.clone()),
        };
        Context { place, card, to }
    }

    /// Opens a place: its draft and its scroll come back, its view is watched (5.6.3, 5.1.5).
    pub(crate) fn open(&mut self, place: String) -> Vec<Effect> {
        if input::shown_nowhere(&place) {
            self.note = Some(format!(
                "✗ {place} is the foreman's and planner's exchange, which the window shows nowhere: `mork ctl history {place}` reads it"
            ));
            return Vec::new();
        }
        self.overlay = None;
        self.replying_to = None;
        self.selected = None;
        self.note = None;
        self.older = None;
        if self.session.place != place {
            self.session.place = place.clone();
            let draft = self.session.drafts.get(&place).cloned().unwrap_or_default();
            self.input = draft_content(&draft);
        }
        let share = self.session.scroll.get(&place).copied().unwrap_or(0.0);
        self.ask_scroll(ScrollOp::To(share));
        self.show_room(&place);
        vec![Effect::Watch(place)]
    }

    /// A card's foreman's DM: `dm` sent, as `/dm <card> foreman` sends it, and its place opened.
    pub(crate) fn open_dm(&mut self, card: &str) -> Vec<Effect> {
        let role = mork_types::reply::Role::Foreman;
        let mut fx = self.act_on(input::pressed(&Command::Dm { card: CardId::new(card), role }));
        fx.extend(self.open(format!("{card}/{}/dm", role.as_str())));
        fx
    }

    /// Asks the rooms column to bring a place's row into view.
    pub(crate) fn show_room(&mut self, place: &str) {
        let n = self.rooms_scroll.as_ref().map_or(1, |(n, _)| n + 1);
        self.rooms_scroll = Some((n, place.to_owned()));
    }

    fn ask_scroll(&mut self, op: ScrollOp) {
        let n = self.scroll.map_or(1, |(n, _)| n + 1);
        self.scroll = Some((n, op));
    }

    /// Ctrl+↑ or Ctrl+↓: the next row of the rooms column as it shows them, from the one picked
    /// or the one standing for the place shown.
    pub(crate) fn select(&mut self, step: i64) -> Vec<Effect> {
        let collapsed = &self.session.collapsed;
        let rows: Vec<String> = crate::rooms::order(&self.screen, collapsed).into_iter().map(str::to_owned).collect();
        if rows.is_empty() {
            return Vec::new();
        }
        let shown = crate::tree::row_of(&self.screen, collapsed, &self.session.place);
        let from = self.selected.as_ref().or(shown.as_ref());
        let at = rows.iter().position(|p| Some(p) == from).map_or(0, |i| i as i64);
        let to = (at + step).clamp(0, rows.len() as i64 - 1) as usize;
        self.selected = rows.get(to).cloned();
        if let Some(picked) = self.selected.clone() {
            self.show_room(&picked);
        }
        Vec::new()
    }

    /// A card folds into its heading, or unfolds (fac-96, fac-168). A row picked under it then is
    /// its heading picked; the open place's line comes into view.
    pub(crate) fn fold_room(&mut self, card: &str, fold: bool) {
        if fold {
            self.session.collapsed.insert(card.to_owned());
        } else {
            self.session.collapsed.remove(card);
        }
        let collapsed = &self.session.collapsed;
        self.selected = self.selected.take().map(|p| crate::tree::row_of(&self.screen, collapsed, &p).unwrap_or(p));
        let kept = self.selected.clone().unwrap_or_else(|| self.session.place.clone());
        self.show_room(&kept);
    }

    // Ctrl+← folds the picked line's card into its heading, Ctrl+→ unfolds it (fac-168).
    fn fold_picked(&mut self, fold: bool) -> bool {
        let Some(picked) = self.selected.as_deref() else { return false };
        let row = self.screen.rooms.iter().find(|r| r.place == picked);
        let Some(card) = row.and_then(|r| r.under.as_ref()).map(|u| u.card.to_string()) else { return false };
        self.fold_room(&card, fold);
        true
    }

    /// Alt+N: the N-th button above the input, pressed (5.7.6).
    pub(crate) fn alt(&mut self, n: u8) -> Vec<Effect> {
        let index = usize::from(n).saturating_sub(1);
        match self.place().and_then(|v| v.offers.get(index)).cloned() {
            Some(o) => {
                let place = self.session.place.clone();
                self.press(None, &place, &o.label, &o.command)
            }
            None => Vec::new(),
        }
    }

    /// ↑ or ↓: the Board's rows on main's Board, else the conversation scrolls three lines
    /// (5.7.8): ↑ toward its oldest message.
    pub(crate) fn arrow(&mut self, step: i64) -> Vec<Effect> {
        if !self.on_board() {
            self.ask_scroll(ScrollOp::By(-(step as f32) * SCROLL_STEP));
            return Vec::new();
        }
        let n = crate::board::rows(&self.screen, &self.board_open).len() as i64;
        if n > 0 {
            let at = self.board_row.map_or(if step > 0 { -1 } else { n }, |i| i as i64);
            self.board_row = Some((at + step).clamp(0, n - 1) as usize);
        }
        Vec::new()
    }

    pub(crate) fn on_board(&self) -> bool {
        self.session.place == "main" && self.session.tab == Tab::Board && self.overlay.is_none()
    }

    /// Sent once, and only if the same offer is still in the latest view of where it was drawn
    /// (5.7.6, 5.9.2, 5.10.4).
    pub(crate) fn press(&mut self, ask: Option<&str>, place: &str, label: &str, command: &Command) -> Vec<Effect> {
        let offers = match ask {
            Some(a) => self.screen.hands.iter().find(|h| h.ask == a).map(|h| h.offers.as_slice()),
            None => self.places.get(place).map(|v| v.offers.as_slice()),
        };
        if !offers.unwrap_or(&[]).iter().any(|o| o.label == label && o.command == *command) {
            self.note = Some(format!("✗ {label} is no longer on offer: nothing sent"));
            return Vec::new();
        }
        self.note = None;
        self.act_on(input::pressed(command))
    }

    /// A pick in an agent's picker (fac-96): sent while the place shown has the agent or its card
    /// the session (fac-105) and offers the model and effort. Models go with their CLI.
    pub(crate) fn pick(&mut self, agent: &str, model: Option<ModelChoice>, effort: Option<Effort>) -> Vec<Effect> {
        let row = self.agent_row(agent);
        if !row.is_some_and(|a| {
            model.as_ref().is_none_or(|m| a.choices.contains(m)) && effort.is_none_or(|e| a.efforts.contains(&e))
        }) {
            let what =
                model.map_or_else(|| effort.map_or_else(|| agent.to_owned(), |e| e.as_str().to_owned()), |m| m.label);
            self.note = Some(format!("✗ {what} is no longer on offer: nothing sent"));
            return Vec::new();
        }
        self.note = None;
        let (cli, model) = model.map(|m| (m.cli.as_str().to_owned(), m.model)).unzip();
        let effort = effort.map(|e| e.as_str().to_owned());
        self.act_on(input::pressed(&Command::Pick { agent: agent.to_owned(), model, effort, cli }))
    }

    /// A button of the top line: sent while the top line still shows it (fac-19).
    pub(crate) fn top_button(&mut self, label: &str, command: &Command) -> Vec<Effect> {
        let shown = crate::spans::top_updates(&self.screen.top);
        if !shown.iter().any(|u| u.button.as_ref().is_some_and(|(l, c)| l == label && c == command)) {
            self.note = Some(format!("✗ {label} is no longer on offer: nothing sent"));
            return Vec::new();
        }
        self.note = None;
        self.act_on(input::pressed(command))
    }

    /// A finding is accepted as its agent wrote it, so a path that leaves the clone opens nothing
    /// (5.7.5); the app reads its symlinks before the editor starts.
    pub(crate) fn editor(&mut self, file: &str, line: i64) -> Vec<Effect> {
        let Some(clone) = self.place().and_then(|v| v.card.as_ref()).and_then(|c| c.clone.clone()) else {
            self.note = Some("✗ this card has no clone to open the file in".into());
            return Vec::new();
        };
        let Some(inside) = within_clone(file) else {
            self.note = Some(format!("✗ {} isn't a file of the clone: nothing opened", crate::text::clean(file)));
            return Vec::new();
        };
        let path = format!("{}/{inside}", clone.trim_end_matches('/'));
        let argv = self
            .settings
            .editor
            .split_whitespace()
            .map(|w| w.replace("{file}", &path).replace("{line}", &line.to_string()))
            .collect();
        vec![Effect::Editor { argv, clone, path }]
    }

    /// A produced file: opened while the place shown lists it (fac-105); the app checks it stays
    /// in its job's `io/` before the viewer starts.
    pub(crate) fn output(&mut self, job: String, name: String) -> Vec<Effect> {
        let listed = self.place().is_some_and(|v| v.outputs.iter().any(|o| o.job.as_str() == job && o.name == name));
        if !listed {
            self.note = Some(format!("✗ {} is no longer listed: nothing opened", crate::text::clean(&name)));
            return Vec::new();
        }
        self.note = None;
        vec![Effect::Output { job, name }]
    }

    /// A message's image (fac-278): opened while a message of the place shown, pinned or loaded,
    /// carries it.
    pub(crate) fn image(&mut self, path: String) -> Vec<Effect> {
        let shown = |v: &mork_types::view::PlaceView| {
            v.pinned.iter().chain(&v.messages).any(|m| m.images.iter().any(|i| i.path == path))
        };
        if !self.place().is_some_and(shown) {
            self.note = Some(format!("✗ {} is no longer shown: nothing opened", crate::text::clean(&path)));
            return Vec::new();
        }
        self.note = None;
        vec![Effect::Image(path)]
    }

    /// A Board row: a card Mork ran opens its room; one it never ran, nothing (fac-37).
    pub(crate) fn board(&mut self, id: &str) -> Vec<Effect> {
        if crate::board::ran(&self.screen, id) {
            self.open(crate::board::room_of(&self.screen, id))
        } else {
            Vec::new()
        }
    }

    /// A project's `send to orchestrator` (fac-230, fac-249): `orchestrator.review`, sent as any button's
    /// command is.
    pub(crate) fn review(&mut self, section: ReviewSection, target: String) -> Vec<Effect> {
        self.note = None;
        self.act_on(input::pressed(&Command::OrchestratorReview { section, target }))
    }

    /// A Board group's heading: the group opens, or folds; the row picked stays picked while drawn.
    pub(crate) fn board_area(&mut self, key: String) {
        let picked =
            |app: &Self| crate::board::rows(&app.screen, &app.board_open).into_iter().map(|r| r.0).collect::<Vec<_>>();
        let was = self.board_row.and_then(|i| picked(self).get(i).cloned());
        if !self.board_open.remove(&key) {
            self.board_open.insert(key);
        }
        self.board_row = was.and_then(|p| picked(self).iter().position(|r| *r == p));
    }

    /// A turn's text or thinking: it unfolds, or folds.
    pub(crate) fn fold_detail(&mut self, key: String) {
        if !self.details_open.remove(&key) {
            self.details_open.insert(key);
        }
    }

    pub(crate) fn top_press(&mut self) -> WindowOp {
        match self.last_top_press.take() {
            Some(t) if self.now_ms.saturating_sub(t) <= DOUBLE_CLICK_MS => WindowOp::ToggleMaximize,
            _ => {
                self.last_top_press = Some(self.now_ms);
                WindowOp::Drag
            }
        }
    }

    /// A click on a message that folds folds or unfolds it alone; nothing else takes one.
    pub(crate) fn fold(&mut self, seq: u64) {
        let place = self.session.place.clone();
        let Some(view) = self.places.get(&place) else { return };
        if !view.messages.iter().any(|m| m.seq == seq && crate::shape::foldable(m)) {
            return;
        }
        let set = self.session.unfolded.entry(place.clone()).or_default();
        if self.session.all_unfolded.remove(&place) {
            set.extend(view.messages.iter().filter(|m| crate::shape::foldable(m)).map(|m| m.seq));
        }
        if !set.remove(&seq) {
            set.insert(seq);
        }
        if set.is_empty() {
            self.session.unfolded.remove(&place);
        }
    }

    /// Tab: every message of the place unfolds, and Tab again folds them all.
    pub(crate) fn fold_all(&mut self) {
        let place = self.session.place.clone();
        self.session.unfolded.remove(&place);
        if !self.session.all_unfolded.remove(&place) {
            self.session.all_unfolded.insert(place);
        }
    }

    pub(crate) fn set_input(&mut self, text: &str) {
        self.input = draft_content(text);
        self.keep_draft();
    }

    /// The input's text kept as the place's draft (5.7.7).
    pub(crate) fn keep_draft(&mut self) {
        let text = self.input.text();
        if text.is_empty() {
            self.session.drafts.remove(&self.session.place);
        } else {
            self.session.drafts.insert(self.session.place.clone(), text);
        }
    }
}

/// A finding's path as read inside its clone (`./a/../b.rs` is `b.rs`), or none when it is
/// absolute, leaves the clone, names nothing, or holds a control character.
pub fn within_clone(file: &str) -> Option<String> {
    if file.starts_with('/') || file.chars().any(char::is_control) {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in file.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
