use factory_types::action::{Action, ActionStatus, ActionWhat};
use serde_json::json;

use super::Poster;
use crate::decide::Intent;

impl Poster<'_> {
    /// An action as this decision has it so far.
    pub fn action(&self, id: &str) -> Option<Action> {
        self.actions.get(id).cloned().or_else(|| self.state.actions.get(id).cloned())
    }

    pub fn put_action(&mut self, a: Action) {
        self.actions.insert(a.id.clone(), a);
    }

    /// Actions in key order, borrowing this decision's version where it overrides the stored one.
    pub fn all_actions(&self) -> impl Iterator<Item = &Action> {
        let mut stored = self.state.actions.iter().peekable();
        let mut changed = self.actions.iter().peekable();
        std::iter::from_fn(move || {
            let next = match (stored.peek(), changed.peek()) {
                (Some((a, _)), Some((b, _))) if a < b => stored.next(),
                (Some((a, _)), Some((b, _))) if a == b => {
                    stored.next();
                    changed.next()
                }
                (_, Some(_)) => changed.next(),
                (Some(_), None) => stored.next(),
                (None, None) => None,
            };
            next.map(|(_, action)| action)
        })
    }

    /// The time of a project's last fetch, the store's included (fac-336).
    pub fn last_fetch(&self, project: &str) -> Option<i64> {
        let prefix = format!("fetch:{project}:");
        let unloaded = self.state.unloaded.actions.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix));
        self.all_actions()
            .filter_map(|a| {
                if let ActionWhat::Fetch { project: p, at } = &a.what { (p == project).then_some(*at) } else { None }
            })
            .chain(unloaded.filter_map(|(k, _)| k[prefix.len()..].split('@').next()?.parse().ok()))
            .max()
    }

    /// A new fetch of a project: its time is after every fetch of it there is, so its natural
    /// key (project, time) never names an earlier fetch, whatever the clock says.
    pub fn fetch(&mut self, project: &str) -> factory_types::ActionId {
        let last = self.last_fetch(project);
        let at = last.map_or(self.now, |l| self.now.max(l + 1));
        self.act(ActionWhat::Fetch { project: project.into(), at })
    }

    /// Decides an outward action (2.5): its natural key first. One already pending, started or
    /// done is the same action, and nothing new is made; its id is returned.
    pub fn act(&mut self, what: ActionWhat) -> factory_types::ActionId {
        self.act_as(what, false)
    }

    /// As `act`, or `anew`, an action of its own, whatever there is alike: a finding raised again
    /// once resolved is commented again, its occurrence told apart by its caller (fac-111).
    pub fn act_as(&mut self, what: ActionWhat, anew: bool) -> factory_types::ActionId {
        let base = what.key();
        // A board write keyed by its content is the same write when it is done too (M3.16); one
        // the engine decides keeps R6's rule: one done is history.
        let board = !what.decided();
        let same = |a: &Action| a.key == base || a.key.starts_with(&format!("{base}@"));
        // A PR opening an older build keyed by its branch alone is this card's own, never another's
        // (fac-382); those the state doesn't hold are of ended cards.
        let legacy = what.legacy_key();
        let theirs = |a: &Action| {
            legacy
                .as_ref()
                .is_some_and(|l| (a.key == *l || a.key.starts_with(&format!("{l}@"))) && a.what.card() == what.card())
        };
        let alike = |a: &&Action| {
            (same(a) || theirs(a)) && a.status != ActionStatus::Dropped && !(board && a.status == ActionStatus::Done)
        };
        if let Some(a) = self.all_actions().find(alike).filter(|_| !anew) {
            return a.id.clone();
        }
        // History the state doesn't hold is done or dropped (fac-336).
        let old = |s: ActionStatus| s != ActionStatus::Dropped && !(board && s == ActionStatus::Done);
        if let Some(id) = self.state.unloaded.alike(&base, old).filter(|_| !anew) {
            return id.clone();
        }
        let taken = self.all_actions().any(same) || self.state.unloaded.key_taken(&base);
        let id = self.next_action_id();
        // The key stays unique in the state: one an earlier action had is made this one's own.
        let key = if taken { format!("{base}@{id}") } else { base };
        let held = what
            .bitbucket()
            .then(|| self.state.world.token_refused.clone())
            .flatten()
            .map(|w| format!("{}{w}", crate::decide::token::TOKEN_HOLD));
        let unwritten = crate::board::retry::proof(&what);
        let a = Action {
            id: id.clone(),
            what,
            key,
            status: if held.is_some() { ActionStatus::Held } else { ActionStatus::Pending },
            result: None,
            words: held.clone(),
            attempts: 0,
            created_ms: self.now,
            done_ms: None,
            cancelled: false,
            door: None,
            unwritten,
            withdrawn: false,
        };
        self.journal("action", a.what.card().map(|c| c.as_str()), json!({"id": id, "key": a.key}));
        if held.is_none() {
            self.intents.push(Intent::RunAction { id: id.clone() });
        }
        self.actions.insert(id.clone(), a);
        id
    }

    /// A new action's id: one above every id there is, stored or not loaded (fac-336), since a count
    /// would reuse an id once there are gaps.
    pub fn next_action_id(&self) -> factory_types::ActionId {
        let top = self
            .state
            .actions
            .keys()
            .chain(self.actions.keys())
            .filter_map(|k| k.strip_prefix("act-")?.parse::<u64>().ok())
            .fold(self.state.unloaded.action_top, u64::max);
        factory_types::ActionId::new(format!("act-{}", top + 1))
    }
}
