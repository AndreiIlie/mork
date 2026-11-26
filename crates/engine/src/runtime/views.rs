// Views (1.5.2): `core`'s screen and each watched place's view, pushed to every watcher after
// each commit, each within its line's bound.

use std::sync::Arc;

use mork_types::chat::MessagesView;
use mork_types::seen::TurnView;
use mork_types::view::{BoardView, Doing, HistoryPage, MessageView, PlaceView, Screen};
use serde_json::{Value, json};

use super::Engine;
use crate::sockets::Conn;

// What a view's messages may take of a line (1.6.1's 1 MiB, with room for the rest).
const VIEW_BUDGET: usize = 600 * 1024;
// A line's bound (1.6.1), less room for the event around the view.
const LINE_BUDGET: usize = 1024 * 1024 - 32 * 1024;
const STEPS_MAX: usize = 200;
const STEP_MAX: usize = 512;
// An agent's step list's steps, its first ones kept (fac-128).
const LIST_MAX: usize = 100;
const WORDS_MAX: usize = 4 * 1024;
// Only the view cuts: the record and the state keep a message's text whole.
const TEXT_MAX: usize = 64 * 1024;
// What a view's agents' turns may take of its line, all together, and each text or thinking.
const TURNS_BUDGET: usize = 192 * 1024;
const DETAIL_MAX: usize = 8 * 1024;
/// What `watch` names to get the screen rather than a place (5).
pub const SCREEN: &str = "screen";

fn cut_to(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}…", &text[..end]), true)
}

// What a view shows of a message, so the line holds.
fn trimmed(mut m: MessageView) -> MessageView {
    let (text, cut) = cut_to(&m.text, TEXT_MAX);
    m.text = text;
    m.cut = m.cut || cut;
    m.headline = cut_to(&m.headline, WORDS_MAX).0;
    if let Some(h) = m.hand.as_mut() {
        h.message = cut_to(&h.message, TEXT_MAX).0;
        h.headline = cut_to(&h.headline, WORDS_MAX).0;
        h.ask = cut_to(&h.ask, WORDS_MAX).0;
    }
    for f in &mut m.findings {
        f.proof = cut_to(&f.proof, WORDS_MAX).0;
    }
    m
}

fn size_of<T: serde::Serialize>(t: &T) -> usize {
    serde_json::to_string(t).map_or(0, |s| s.len())
}

// The newest messages that fit in `budget`, oldest first, each as a view shows it.
fn newest_within(all: &[MessageView], budget: usize) -> Vec<MessageView> {
    let mut kept: Vec<MessageView> = Vec::new();
    let mut size = 0usize;
    for m in all.iter().rev().cloned().map(trimmed) {
        size += size_of(&m);
        if size > budget {
            break;
        }
        kept.push(m);
    }
    kept.reverse();
    kept
}

// A reply's own words are never cut: only what a view shows of them, so the line holds.
fn bounded(mut v: PlaceView) -> PlaceView {
    let per_agent = STEPS_MAX / v.agents.len().max(1);
    let turn_budget = TURNS_BUDGET / v.agents.len().max(1);
    for a in &mut v.agents {
        if let Some(t) = a.turn.as_mut() {
            bounded_turn(t, per_agent, turn_budget);
        }
        a.steps = a.steps.split_off(a.steps.len().saturating_sub(per_agent));
        a.steps.iter_mut().for_each(|s| *s = cut_to(s, STEP_MAX).0);
    }
    for l in &mut v.lists {
        l.steps.truncate(LIST_MAX);
        l.steps.iter_mut().for_each(|s| s.words = cut_to(&s.words, STEP_MAX).0);
    }
    let review_seats =
        v.review.iter_mut().flat_map(|r| r.foreman.iter_mut().chain(r.reviewers.iter_mut()).chain(r.others.iter_mut()));
    for a in v.agents.iter_mut().chain(v.sessions.iter_mut()).chain(review_seats) {
        match &mut a.doing {
            Doing::Working { step: w } | Doing::Error { words: w } => *w = cut_to(w, WORDS_MAX).0,
            Doing::WaitingForSlot | Doing::WaitingOnYou | Doing::Idle => {}
        }
    }
    if let Some(t) = v.triage.as_mut() {
        for f in &mut t.findings {
            f.proof = cut_to(&f.proof, WORDS_MAX).0;
        }
    }
    let overview = v.overview.take();
    let all = std::mem::take(&mut v.messages);
    v.pinned = std::mem::take(&mut v.pinned).into_iter().map(trimmed).collect();
    let left = LINE_BUDGET.saturating_sub(size_of(&v)).saturating_sub(super::chat::BUDGET);
    let (overview, msg_budget) = match overview {
        Some(mut o) => {
            // Give overview its own share of the line budget so it never starves the room's messages (fac-684).
            bound_round_proofs(&mut o, (VIEW_BUDGET / 2).min(left / 2));
            let rem = left.saturating_sub(size_of(&o));
            (Some(o), rem)
        }
        None => (None, left),
    };
    let (kept, cut) = fit_messages(all, &mut v.pinned, msg_budget);
    v.earlier += cut;
    v.messages = kept;
    v.overview = overview;
    v
}

// Cut round proofs to WORDS_MAX and shed oldest rounds' proofs first if the overview still exceeds its budget (fac-698).
fn bound_round_proofs(o: &mut mork_types::view::OverviewView, budget: usize) {
    for f in o.stages.iter_mut().filter_map(|s| s.round.as_mut()).flat_map(|r| r.findings.iter_mut()) {
        f.proof = cut_to(&f.proof, WORDS_MAX).0;
    }
    if size_of(o) <= budget {
        return;
    }
    for i in 0..o.stages.len() {
        let Some(round) = o.stages[i].round.as_mut() else { continue };
        for f in &mut round.findings {
            f.proof.clear();
        }
        if size_of(o) <= budget {
            break;
        }
    }
}

fn bounded_messages(mut v: MessagesView) -> MessagesView {
    let all = std::mem::take(&mut v.messages);
    v.pinned = std::mem::take(&mut v.pinned).into_iter().map(trimmed).collect();
    let left = LINE_BUDGET.saturating_sub(size_of(&v));
    let (kept, cut) = fit_messages(all, &mut v.pinned, left);
    v.earlier += cut;
    v.messages = kept;
    v
}

// The budget's cut hides no open ask either (fac-42).
fn fit_messages(all: Vec<MessageView>, pinned: &mut Vec<MessageView>, left: usize) -> (Vec<MessageView>, usize) {
    let kept = newest_within(&all, VIEW_BUDGET.saturating_sub(size_of(pinned)).min(left));
    let cut = all.len() - kept.len();
    pinned.extend(all.into_iter().take(cut).filter(|m| m.ask_open).map(trimmed));
    (kept, cut)
}

// Matches web/src/Turns.tsx `STEPS_SHOWN`: older steps shed details before recent visible steps do.
const RECENT_STEPS: usize = 8;

// What a view shows of a turn: its latest steps, each detail cut, and past its share of the line
// older hidden steps shed details first, then recent thinking, then recent narration text.
pub(super) fn bounded_turn(t: &mut TurnView, steps: usize, budget: usize) {
    let gone = t.steps.len().saturating_sub(steps);
    let mut cut = gone > 0;
    t.steps.drain(..gone);
    for s in &mut t.steps {
        s.words = cut_to(&s.words, STEP_MAX).0;
        if let Some(c) = &mut s.command {
            let (text, cut) = crate::cli::turn::cut_to(&c.text, DETAIL_MAX);
            c.text = text;
            c.cut |= cut;
        }
    }
    for d in t.steps.iter_mut().flat_map(|s| s.details.iter_mut()).chain(t.after.iter_mut()) {
        let (text, c) = cut_to(&d.text, DETAIL_MAX);
        (d.text, d.cut) = (text, d.cut || c);
    }
    let mut size = size_of(t);
    let split = t.steps.len().saturating_sub(RECENT_STEPS);
    let thinking = Some(mork_types::seen::DetailKind::Thinking);
    cut |= evict_slice(&mut t.steps[..split], None, &mut size, budget, thinking);
    cut |= evict_slice(&mut t.steps[..split], None, &mut size, budget, None);
    cut |= evict_commands(&mut t.steps[..split], &mut size, budget);
    cut |= evict_slice(&mut t.steps[split..], Some(&mut t.after), &mut size, budget, thinking);
    cut |= evict_slice(&mut t.steps[split..], Some(&mut t.after), &mut size, budget, None);
    cut |= evict_commands(&mut t.steps[split..], &mut size, budget);
    t.cut |= cut;
}

fn evict_commands(steps: &mut [mork_types::seen::StepView], size: &mut usize, budget: usize) -> bool {
    let mut cut = false;
    for command in steps.iter_mut().filter_map(|s| s.command.as_mut()) {
        if *size <= budget {
            break;
        }
        let before = size_of(command);
        command.text.clear();
        command.cut = true;
        *size = size.saturating_sub(before).saturating_add(size_of(command));
        cut = true;
    }
    cut
}

fn evict_slice(
    steps: &mut [mork_types::seen::StepView],
    after: Option<&mut Vec<mork_types::seen::Detail>>,
    size: &mut usize,
    budget: usize,
    only: Option<mork_types::seen::DetailKind>,
) -> bool {
    let mut cut = false;
    for details in steps.iter_mut().map(|s| &mut s.details).chain(after) {
        let mut i = 0;
        while *size > budget && i < details.len() {
            if only.is_none_or(|k| details[i].kind == k) {
                *size = size.saturating_sub(size_of(&details.remove(i)));
                cut = true;
            } else {
                i += 1;
            }
        }
    }
    cut
}

// A page past its line keeps the messages nearest the view: the next page reaches the rest.
fn bounded_page(mut p: HistoryPage) -> HistoryPage {
    let all = std::mem::take(&mut p.messages);
    p.messages = newest_within(
        &all,
        VIEW_BUDGET.min(LINE_BUDGET.saturating_sub(size_of(&p)).saturating_sub(super::chat::BUDGET)),
    );
    p.earlier += all.len() - p.messages.len();
    p
}

// A board's lists of rows that the line's bound may shorten.
#[derive(Debug, Clone, Copy)]
enum Rows {
    Done,
    Area(usize),
    Live,
    InLine,
}

fn lists(b: &BoardView) -> impl Iterator<Item = Rows> {
    [Rows::Done, Rows::Live, Rows::InLine].into_iter().chain((0..b.areas.len()).map(Rows::Area))
}

fn len(b: &BoardView, r: Rows) -> usize {
    match r {
        Rows::Done => b.done.cards.len(),
        Rows::Area(i) => b.areas.get(i).map_or(0, |a| a.cards.len()),
        Rows::Live => b.live.cards.len(),
        Rows::InLine => b.merge.waiting.len(),
    }
}

// A quarter of a list's rows go: a group's lowest ids, a queue's furthest from its turn.
fn shorten(b: &mut BoardView, r: Rows) {
    let keep = len(b, r) - len(b, r).div_ceil(4);
    match r {
        Rows::Done => drop(b.done.cards.drain(..b.done.cards.len() - keep)),
        Rows::Area(i) => {
            if let Some(a) = b.areas.get_mut(i) {
                drop(a.cards.drain(..a.cards.len() - keep));
            }
        }
        Rows::Live => b.live.cards.truncate(keep),
        Rows::InLine => b.merge.waiting.truncate(keep),
    }
}

fn bounded_screen(mut s: Screen) -> Screen {
    for h in &mut s.hands {
        // Large triage asks list every finding for the native window; keep them whole until LINE_BUDGET trims (fac-680).
        if h.kind != mork_types::ask::AskKind::Triage {
            h.words = cut_to(&h.words, WORDS_MAX).0;
        }
    }
    let size = |s: &Screen| serde_json::to_string(s).map_or(0, |t| t.len());
    let mut done_only = true;
    while size(&s) > LINE_BUDGET {
        // The done rows go first, then the longest list's.
        let longest = s
            .boards
            .iter()
            .enumerate()
            .flat_map(|(i, b)| lists(b).map(move |r| (i, r, len(b, r))))
            .filter(|(_, r, n)| *n > 0 && (!done_only || matches!(r, Rows::Done)))
            .max_by_key(|(_, _, n)| *n);
        match longest {
            Some((i, r, _)) => shorten(&mut s.boards[i], r),
            None if done_only => done_only = false,
            None => break,
        }
    }
    if size(&s) > LINE_BUDGET {
        for h in &mut s.hands {
            h.words = cut_to(&h.words, 256).0;
        }
    }
    while size(&s) > LINE_BUDGET && s.hands.pop().is_some() {}
    s
}

impl Engine {
    pub(super) fn ctx_now(&self) -> mork_core::decide::Ctx<'_> {
        mork_core::decide::Ctx { now: crate::clock::now_ms(), config: &self.ctx.config, faults: self.faults() }
    }

    /// A place's view, its latest `tail` messages when one is given, as `core` works it out,
    /// within its line; without `full`, only the conversation (fac-280).
    pub(super) fn view(&self, place: &str, tail: Option<u64>, full: bool) -> Value {
        if !full {
            let v = mork_core::views::conversation::messages(&self.state, place, tail);
            return serde_json::to_value(bounded_messages(v)).unwrap_or_default();
        }
        let card = mork_core::messages::places::card_of(&self.state, place);
        let outside = self.outside(card.as_ref());
        let v = mork_core::views::place::place(&self.state, place, tail, &outside, self.ctx_now());
        let mut v = bounded(v);
        v.turns = self.chat_turns(place, &v.messages, None);
        super::chat::bound(&mut v.turns);
        serde_json::to_value(v).unwrap_or_default()
    }

    /// `history`: a page of a place's messages before one, within its line (fac-42); the turns
    /// between them only with `turns` (fac-280).
    pub(super) fn history(&self, place: &mork_types::PlaceId, before: u64, count: u64, turns: Option<bool>) -> Value {
        let page = mork_core::views::conversation::history(&self.state, place.as_str(), before, count);
        let mut page = bounded_page(page);
        if turns.unwrap_or(false) {
            page.turns = self.chat_turns(place.as_str(), &page.messages, Some(before));
            super::chat::bound(&mut page.turns);
        }
        json!({"history": page})
    }

    /// The screen but the place shown (5).
    pub(super) fn screen_view(&self) -> Value {
        let outside = self.outside(None);
        let s = mork_core::views::screen::screen(&self.state, &outside, self.ctx_now());
        serde_json::to_value(bounded_screen(s)).unwrap_or_default()
    }

    /// `usage`: every limit, context and 7-day burn summary, in full (5.10.3).
    pub(super) fn usage(&self) -> Value {
        let now = crate::clock::now_ms();
        let accounts = &self.views_cache.claude_accounts;
        let (lines, limits) = super::usage::answer(&self.state, now, accounts);
        let contexts = self.agent_contexts();
        let at = (now, self.views_cache.tz_offset_min);
        let recent_burn = super::burn::recent(&self.state, &self.store, at, accounts);
        // The answer's keys print sorted: `by_pool` sorts first, so the pace lines come above the full fields.
        json!({"usage": {"by_pool": lines, "limits": limits, "contexts": contexts, "recent_burn": recent_burn}})
    }

    fn agent_contexts(&self) -> Vec<Value> {
        self.state
            .agents
            .values()
            .filter_map(|a| {
                if let Some(c) = self.seen.get(&a.key).and_then(|s| s.context.as_ref()) {
                    let window = self.ctx.config.mork.effective_window(c.window);
                    return Some(json!({"agent": a.key.as_str(), "used": c.used, "window": window, "model": c.model}));
                }
                let stored = mork_core::agents::costs::stored_context(&self.ctx.config, a)?;
                let model = stored.model.map(str::to_owned).or_else(|| {
                    mork_core::agents::models::setting(&self.state, &self.ctx.config, a).map(|s| s.model.model)
                });
                Some(json!({"agent": a.key.as_str(), "used": stored.used, "window": stored.window, "model": model}))
            })
            .collect()
    }

    /// Each watched place's view, and the screen when it is watched.
    pub(super) fn send_views(&self, conn: &Arc<Conn>, watched: &super::Watched) {
        for p in &watched.places {
            let (key, mut view) = if p == SCREEN {
                ("screen", self.screen_view())
            } else {
                ("view", self.view(p, watched.tail, watched.full))
            };
            if watched.unchanged(p, &mut view) {
                continue;
            }
            conn.send_line(&json!({"event": {key: view}}).to_string());
        }
    }

    /// The screen alone, to the watchers of it: for what moves every few seconds and touches no place.
    pub(super) fn publish_screen(&mut self) {
        self.watchers.retain(|(c, _)| c.is_open());
        for (conn, watched) in &self.watchers {
            if watched.places.iter().any(|p| p == SCREEN) {
                let mut view = self.screen_view();
                if !watched.unchanged(SCREEN, &mut view) {
                    conn.send_line(&json!({"event": {"screen": view}}).to_string());
                }
            }
        }
    }

    /// After a commit: every watcher gets the views of its places that changed (fac-321).
    pub(super) fn publish(&mut self) {
        self.views_cache.stale = false;
        self.views_cache.sent = Some(crate::clock::instant());
        self.watchers.retain(|(c, _)| c.is_open());
        let watchers = self.watchers.clone();
        for (conn, watched) in &watchers {
            self.send_views(conn, watched);
        }
    }
}
