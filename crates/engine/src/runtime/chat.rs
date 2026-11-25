// Chat reads the same retained streams as the job log (fac-138); only ended reads are cached.

use std::collections::BTreeMap;
use std::sync::Arc;

use factory_types::JobId;
use factory_types::chat::ChatTurn;
use factory_types::seen::TurnView;
use factory_types::view::MessageView;

use super::Engine;

pub(super) const BUDGET: usize = 192 * 1024;

const CACHED: usize = factory_core::views::conversation::MESSAGES_MAX;

type Ended = BTreeMap<JobId, Option<Arc<Log>>>;

#[derive(Debug, Default)]
pub(super) struct Cache {
    places: BTreeMap<String, Ended>,
}

#[derive(Debug)]
struct Log {
    step_count: usize,
    turn: TurnView,
}

impl Log {
    fn reduced(mut turn: TurnView) -> Self {
        let step_count = turn.steps.len();
        bounded_turn(&mut turn, BUDGET);
        Self { step_count, turn }
    }
}

impl Cache {
    fn read(&mut self, place: &str, id: &JobId, read: impl FnOnce() -> Option<TurnView>) -> Option<Arc<Log>> {
        let ended = self.places.entry(place.to_owned()).or_default();
        if let Some(log) = ended.get(id) {
            return log.clone();
        }
        let log = read().map(Log::reduced).map(Arc::new);
        while ended.len() >= CACHED {
            ended.pop_first();
        }
        ended.insert(id.clone(), log.clone());
        log
    }

    fn forget(&mut self, place: &str, id: &JobId) {
        if let Some(ended) = self.places.get_mut(place) {
            ended.remove(id);
        }
    }
}

impl Engine {
    pub(super) fn chat_turns(&self, place: &str, messages: &[MessageView], before: Option<u64>) -> Vec<ChatTurn> {
        factory_core::views::conversation::turns(&self.state, place, messages, before)
            .into_iter()
            .filter_map(|mut t| {
                let log = self.chat_log(place, &t.turn.job, t.running)?;
                let mut turn = log.turn.clone();
                if !t.running {
                    turn.read.retain(|r| t.turn.read.iter().any(|saved| saved.seq == r.seq));
                    for saved in &t.turn.read {
                        if !turn.read.iter().any(|r| r.seq == saved.seq) {
                            turn.read.push(saved.clone());
                        }
                    }
                    turn.read.sort_by_key(|r| (r.step, r.seq));
                }
                t.turn = turn;
                t.step_count = log.step_count;
                Some(t)
            })
            .collect()
    }

    fn chat_log(&self, place: &str, id: &JobId, running: bool) -> Option<Arc<Log>> {
        if running {
            let turn = self.live.get(id).map(|l| crate::cli::turn::turn_of(&l.stream, id));
            let turn = turn.unwrap_or_else(|| TurnView { job: id.clone(), ..TurnView::default() });
            return Some(Arc::new(Log::reduced(turn)));
        }
        let mut cache = self.views_cache.chat.borrow_mut();
        if !self.job_dir(id).out().exists() {
            cache.forget(place, id);
            return None;
        }
        cache.read(place, id, || self.retained_turn(id))
    }
}

pub(super) fn bound(turns: &mut [ChatTurn]) {
    let budget = BUDGET / turns.len().max(1);
    for t in turns {
        let wrap = serde_json::to_vec(t)
            .map_or(0, |s| s.len())
            .saturating_sub(serde_json::to_vec(&t.turn).map_or(0, |s| s.len()));
        bounded_turn(&mut t.turn, budget.saturating_sub(wrap));
        let mut size = serde_json::to_vec(t).map_or(0, |s| s.len());
        let mut gone = 0;
        for step in &t.turn.steps {
            if size <= budget {
                break;
            }
            size = size.saturating_sub(serde_json::to_vec(step).map_or(0, |s| s.len()) + 1);
            gone += 1;
        }
        t.turn.steps.drain(..gone);
        shift_read(&mut t.turn, gone);
        t.turn.cut |= gone > 0;
    }
}

fn bounded_turn(turn: &mut TurnView, budget: usize) {
    let before = turn.steps.len();
    super::views::bounded_turn(turn, 200, budget);
    let gone = before.saturating_sub(turn.steps.len());
    shift_read(turn, gone);
}

fn shift_read(turn: &mut TurnView, gone: usize) {
    for point in &mut turn.read {
        point.step = point.step.saturating_sub(gone);
    }
}
