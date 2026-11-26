//! The window keeps only its own state (5.1.5); every action becomes an `Effect` the app
//! carries out (section 5).

use std::collections::{BTreeMap, BTreeSet};

use iced::Element;
use iced::widget::text_editor;
use mork_types::Command;
use mork_types::config::{Effort, TunnelConfig};
use mork_types::view::{AgentRow, ModelChoice, PlaceView, Screen};
use serde::{Deserialize, Serialize};

use crate::input::draft_content;
use crate::session::{Tab, UiSession};
use mork_types::web_tunnel::Status as TunnelStatus;

/// The window's settings from the config (1.14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// `[mork] editor`: `code -g {file}:{line}`.
    pub editor: String,
    /// `[mork] animations`.
    pub animations: bool,
    /// `[tunnels.<name>]`: window-owned SSH web forwards (fac-721).
    #[serde(default)]
    pub tunnels: BTreeMap<String, TunnelConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send a command, exactly once (5.7.6).
    Send(Command),
    Watch(String),
    Window(WindowOp),
    /// Run the config's editor on a file of a card's clone (5.7.5): `argv`, on `path`, a file
    /// of `clone` once its symlinks are read, or nothing.
    Editor {
        argv: Vec<String>,
        clone: String,
        path: String,
    },
    /// Put a conversation message's words on the desktop's clipboard (fac-33).
    Copy(String),
    /// Open a web page outside the window.
    Link(String),
    Tunnel(String, bool),
    /// Opens `name` in job `job`'s `io/` once the app checked it stays there (fac-105); a refusal
    /// comes back as `Message::OutputRefused`.
    Output {
        job: String,
        name: String,
    },
    /// Open a message's image in the system's viewer (fac-278), once the app checked it is a PNG or
    /// JPEG a job published or has in its `io/`, or a pasted one; a refusal comes back as `Message::OutputRefused`.
    Image(String),
    /// The window's own state changed: write `ui-session.json`.
    SaveSession(UiSession),
    /// The input's paste in a place (fac-31): the clipboard's text pasted as the editor pastes
    /// it, or with no text its image saved as a file, given back as `Message::Pasted` for `place`.
    Paste {
        place: String,
    },
}

/// The window's own controls (5.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOp {
    Drag,
    ToggleMaximize,
    Minimize,
    Close,
}

/// A scroll the window asks of the conversation, which stays anchored at its latest message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollOp {
    /// By this many px toward its oldest message (↑ is positive, ↓ negative).
    By(f32),
    /// To this share of the way from its latest message to its oldest, 0 to 1 (5.1.5).
    To(f32),
    /// Into view: this band of a list, in px from its top (the open or picked room, 5.6.3).
    Show { top: f32, bottom: f32 },
}

/// A key the window acts on, past the input's own editing (5.6.3, 5.7, 5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tab,
    Esc,
    /// Enter: send the draft, or open the selected room when it is empty.
    Enter,
    CtrlUp,
    CtrlDown,
    CtrlLeft,
    CtrlRight,
    /// Alt+1…Alt+9: the N-th button.
    Alt(u8),
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub enum Message {
    Key(Key),
    Edit(text_editor::Action),
    /// The input's paste key.
    Paste,
    /// The clipboard's image pasted in `place`, saved: its file's path, or why there is none
    /// (fac-31).
    Pasted {
        place: String,
        image: Result<String, String>,
    },
    /// A pasted image's ×: it leaves the draft.
    Unpaste(usize),
    /// Send now: the draft sent, cutting its recipient's running turn (fac-78).
    SendNow,
    /// A click on a place's row or `open`.
    Open(String),
    /// Opening a hand with its ask selected as the reply target.
    OpenAsk {
        place: String,
        ask: String,
    },
    /// A card's `dm foreman` tab: its foreman's DM, opened as `/dm` opens it (fac-96).
    OpenDm(String),
    /// Main's tabs.
    Tab(Tab),
    Lookback(mork_types::ProjectId, mork_types::lookback::Read),
    /// A message's line: fold or unfold it.
    Fold(u64),
    /// A message's `copy`, by its number (fac-33).
    Copy(u64),
    /// A card heading's fold in the rooms: all under it folds into it, or unfolds (fac-96, fac-168).
    FoldRoom(String),
    /// A button pressed: the offer it showed (its words and command), and where it was drawn:
    /// the place whose view offered it, and the ask whose hand did, if a hand's (5.7.6, 5.9.2).
    Press {
        ask: Option<String>,
        place: String,
        label: String,
        command: Box<Command>,
    },
    /// A card's switch flipped (fac-258): the place it was drawn in, and the command it showed.
    Switch(String, Box<Command>),
    /// A button of the top line's update (fac-19): its words and the command it sends.
    TopButton {
        label: String,
        command: Box<Command>,
    },
    Finding {
        file: String,
        line: i64,
    },
    BoardRow(String),
    /// A Board group's heading: it opens or folds, by its key (`areas::key`).
    BoardArea(String),
    Window(WindowOp),
    /// A press on the top line: a drag, or with the one before it a double click.
    TopPress,
    /// The conversation scrolled: its share of the way from the bottom.
    Scrolled(f32),
    /// The conversation's top came into view, its oldest message shown this one (fac-42).
    Older(u64),
    /// The window moved, to this position on the desktop (its monitor, 5.1.1).
    Moved(i32, i32),
    /// A deliverable's row in the plan, by its index: its tasks fold, or unfold (fac-128).
    PlanFold(usize),
    /// An agent's model and effort clicked: its picker opens over the place (fac-96).
    Picker(String),
    /// A side column's section header: it folds or opens (fac-105).
    Section(crate::side::Section),
    ClaudeAccount(String),
    /// A project's part of a section, `send to orchestrator`: the operator sends it its orchestrator to review
    /// (fac-230, fac-249).
    Review(mork_types::orchestrator::ReviewSection, String),
    /// A card's pull request: its page opens outside the window (fac-105).
    Link(String),
    /// A web address in the conversation opens in the browser (fac-152).
    ChatLink(String),
    /// A file a card's agent turn produced: it opens in the system's viewer (fac-105).
    Output {
        job: String,
        name: String,
    },
    /// A message's image clicked: it opens in the system's viewer (fac-278).
    Image(String),
    /// Why the app didn't hand a produced file to the viewer (fac-105).
    OutputRefused(String),
    ShowTunnels,
    Tunnel(String, bool),
    TunnelStatus(String, TunnelStatus),
    TunnelPage(String),
    /// An agent's steps clicked: its latest turn opens over the place (fac-96).
    Turn(String),
    /// A step's text or thinking (`turn::key`), or a chat's turn block by its job: fold or unfold it.
    Detail(String),
    /// A model, with its CLI, or an effort picked for an agent's next turn.
    Pick {
        agent: String,
        model: Option<ModelChoice>,
        effort: Option<Effort>,
    },
}

/// What shows in the place's area until Esc (5.10.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Help,
    Tunnels,
    /// `/usage`, and its answer once it came.
    Usage {
        answer: Option<serde_json::Value>,
    },
    /// An agent's picker, by its key (fac-96), and the half pair it holds (fac-115).
    Pick {
        agent: String,
        draft: crate::picker::Draft,
    },
    /// An agent's latest turn, by its key (fac-96).
    Turn {
        agent: String,
    },
}

/// Two presses on the top line this close together are a double click.
pub(crate) const DOUBLE_CLICK_MS: i64 = 400;
/// A step of ↑ or ↓: three lines.
pub(crate) const SCROLL_STEP: f32 = 48.0;
/// A place's view starts with its latest this many messages, and each scroll to its top brings
/// this many more (fac-42).
pub const PAGE: u64 = 50;

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub places: BTreeMap<String, PlaceView>,
    pub session: UiSession,
    pub settings: Settings,
    /// The animations' clock, in ms (5.11): frozen in tests.
    pub now_ms: i64,
    /// What the last line or button said went wrong, shown above the input.
    pub note: Option<String>,
    /// `/help` or `/usage`, shown in the place's area until Esc.
    pub overlay: Option<Overlay>,
    /// The asks seen raised, and when the window first saw each (5.11.2).
    pub raised: BTreeMap<String, i64>,
    pub input: text_editor::Content,
    pub(crate) replying_to: Option<String>,
    /// The row picked with Ctrl+↑ and Ctrl+↓, by its place, until Enter opens it (5.6.3).
    pub selected: Option<String>,
    /// The Board's row picked with ↑ and ↓ (5.8.1).
    pub board_row: Option<usize>,
    /// The Board's groups open, by their keys: each starts folded (fac-37).
    pub board_open: BTreeSet<String>,
    /// The turns' texts and thinking, and the chat's turn blocks by job, unfolded: each starts folded.
    pub details_open: BTreeSet<String>,
    /// The latest scroll asked of the conversation, by its number: the conversation's wrapper
    /// runs each once (`ui::wrappers`).
    pub scroll: Option<(u64, ScrollOp)>,
    /// The latest room asked into view, by its number: the open or picked room, kept in view by
    /// the list when it is longer than its space (5.6.1, 5.6.3).
    pub rooms_scroll: Option<(u64, String)>,
    /// The oldest message shown when more were last asked for: once each (fac-42).
    pub older: Option<u64>,
    /// By card, the deliverables a click folded or unfolded in its plan: open or not (fac-128).
    pub plan_folds: BTreeMap<String, crate::plan::Folds>,
    pub tunnels: BTreeMap<String, TunnelStatus>,
    pub(crate) last_top_press: Option<i64>,
}

impl App {
    pub fn new(screen: Screen, session: UiSession, settings: Settings) -> Self {
        let raised = screen.hands.iter().map(|h| (h.ask.to_string(), i64::MIN)).collect();
        let input = draft_content(session.drafts.get(&session.place).map_or("", String::as_str));
        let share = session.scroll.get(&session.place).copied().unwrap_or(0.0);
        Self {
            rooms_scroll: Some((1, session.place.clone())),
            older: None,
            plan_folds: BTreeMap::new(),
            tunnels: Default::default(),
            scroll: Some((1, ScrollOp::To(share))),
            now_ms: screen.now_ms,
            screen,
            places: BTreeMap::new(),
            session,
            settings,
            note: None,
            overlay: None,
            raised,
            input,
            replying_to: None,
            selected: None,
            board_row: None,
            board_open: BTreeSet::new(),
            details_open: BTreeSet::new(),
            last_top_press: None,
        }
    }

    /// The clock moves at least to when the screen was made (fac-168). The first screen's asks
    /// were raised before the window opened, and don't blink.
    pub fn set_screen(&mut self, screen: Screen) {
        self.now_ms = self.now_ms.max(screen.now_ms);
        let first = self.screen.revision == 0;
        let seen = if first { i64::MIN } else { self.now_ms };
        // The open or picked room comes into view again when the rooms move it or change its
        // space, and when the first screen with the rooms arrives (W13, M4.15).
        let kept = self.selected.clone().unwrap_or_else(|| self.session.place.clone());
        let (old, new) = (&self.screen, &screen);
        let collapsed = &self.session.collapsed;
        if first || crate::rooms::slot(old, collapsed, &kept) != crate::rooms::slot(new, collapsed, &kept) {
            self.show_room(&kept);
        }
        let open: Vec<String> = screen.hands.iter().map(|h| h.ask.to_string()).collect();
        self.raised.retain(|ask, _| open.contains(ask));
        for ask in open {
            self.raised.entry(ask).or_insert(seen);
        }
        self.screen = screen;
    }

    /// The engine's latest view of a place, under the pages loaded before it (fac-42).
    pub fn set_place(&mut self, view: PlaceView) {
        let place = view.place.to_string();
        let old = self.places.remove(&place);
        self.places.insert(place, crate::paging::merged(old, view));
    }

    /// An answer came back: a refusal is said above the input; `/usage`'s (`{"usage": …}`) shows
    /// over the place, `history`'s joins its place's view, and any other is taken as done.
    pub fn answered(&mut self, result: Result<serde_json::Value, String>) {
        match result {
            // Only the `usage` read's answer fills its overlay: another command's answer that
            // comes back meanwhile (an `approve` pressed before `/usage`) isn't it.
            Ok(v) => {
                let page = v.get("history").and_then(|p| serde_json::from_value(p.clone()).ok());
                if let Some(page) = page {
                    self.page(page);
                }
                if let (Some(Overlay::Usage { answer: answer @ None }), Some(usage)) =
                    (&mut self.overlay, v.get("usage"))
                {
                    *answer = Some(usage.clone());
                }
            }
            Err(e) => self.note = Some(format!("✗ {}", crate::text::clean(&e))),
        }
    }

    pub fn update(&mut self, message: Message) -> Vec<Effect> {
        let before = self.session.clone();
        let mut out = self.act(message);
        if self.session != before {
            out.push(Effect::SaveSession(self.session.clone()));
        }
        out
    }

    pub fn view(&self) -> Element<'_, Message> {
        crate::window::frame(self)
    }

    /// The place shown's view, once the engine sent it.
    pub fn place(&self) -> Option<&PlaceView> {
        self.places.get(&self.session.place)
    }

    /// An agent's row in the place shown: one of its own agents', else one of its card's
    /// sessions' (fac-105).
    pub fn agent_row(&self, agent: &str) -> Option<&AgentRow> {
        let v = self.place()?;
        v.agents.iter().chain(&v.sessions).find(|a| a.agent.as_str() == agent)
    }

    /// Whether a message of the place shown is unfolded (5.7.5).
    pub fn unfolded(&self, seq: u64) -> bool {
        let place = &self.session.place;
        self.session.all_unfolded.contains(place) || self.session.unfolded.get(place).is_some_and(|s| s.contains(&seq))
    }

    fn act(&mut self, message: Message) -> Vec<Effect> {
        match message {
            Message::Key(k) => self.key(k),
            Message::Paste if self.takes_input() => vec![Effect::Paste { place: self.session.place.clone() }],
            Message::Paste => Vec::new(),
            Message::SendNow => self.send_now(),
            Message::Open(place) => self.open(place),
            Message::OpenAsk { place, ask } => {
                let effects = self.open(place);
                self.replying_to = Some(ask);
                effects
            }
            Message::OpenDm(card) => self.open_dm(&card),
            Message::Press { ask, place, label, command } => self.press(ask.as_deref(), &place, &label, &command),
            Message::TopButton { label, command } => self.top_button(&label, &command),
            Message::Switch(place, command) => self.switch(&place, &command),
            Message::ClaudeAccount(name) => vec![Effect::Send(Command::ClaudeAccount { name })],
            Message::Finding { file, line } => self.editor(&file, line),
            Message::Output { job, name } => self.output(job, name),
            Message::Image(path) => self.image(path),
            Message::BoardRow(id) => self.board(&id),
            Message::Review(section, target) => self.review(section, target),
            Message::Lookback(project, read) => self.lookback(project, read),
            Message::Window(op) => vec![Effect::Window(op)],
            Message::TopPress => vec![Effect::Window(self.top_press())],
            Message::Older(first) => self.older_page(first),
            Message::Pick { agent, model, effort } => self.picked(&agent, model, effort),
            Message::Link(url) => self.link(url),
            Message::ChatLink(url) => vec![Effect::Link(url)],
            Message::Tunnel(name, open) => self.tunnel(name, open),
            Message::TunnelPage(name) => self.tunnel_page(&name),
            Message::Copy(seq) => self.copy(seq),
            other => {
                self.shift(other);
                Vec::new()
            }
        }
    }

    // The messages that change only what the window shows and keeps.
    fn shift(&mut self, message: Message) {
        match message {
            Message::Edit(action) if self.takes_input() => {
                self.input.perform(action);
                self.keep_draft();
            }
            Message::Pasted { place, image } => self.pasted(place, image),
            Message::Unpaste(i) => self.unpaste(i),
            Message::OutputRefused(why) => self.note = Some(format!("✗ {}", crate::text::clean(&why))),
            Message::ShowTunnels => self.overlay = Some(Overlay::Tunnels),
            Message::TunnelStatus(name, status) => {
                self.tunnels.insert(name, status);
            }
            Message::Tab(tab) => self.session.tab = tab,
            Message::Fold(seq) => self.fold(seq),
            Message::FoldRoom(card) => self.fold_room(&card, !self.session.collapsed.contains(&card)),
            Message::BoardArea(key) => self.board_area(key),
            Message::Scrolled(share) => {
                self.session.scroll.insert(self.session.place.clone(), share);
            }
            Message::Moved(x, y) => self.session.position = Some((x, y)),
            Message::PlanFold(d) => self.fold_deliverable(d),
            Message::Picker(agent) => self.overlay = Some(Overlay::Pick { agent, draft: Default::default() }),
            Message::Section(s) => self.fold_section(s),
            Message::Turn(agent) => self.overlay = Some(Overlay::Turn { agent }),
            Message::Detail(key) => self.fold_detail(key),
            _ => {}
        }
    }
}
