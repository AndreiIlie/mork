// Preview workers (fac-196) run once Andrei turned them on (fac-222) and end when he turns them off or the card is
// no longer live. Kept in memory, so an app's start starts again each he turned on.

use std::collections::{BTreeMap, BTreeSet};

use factory_core::decide::Event;
use factory_core::lifecycle::preview::{self, Action};
use factory_types::Command;
use factory_types::protocol::{ErrorBody, ErrorCode};
use factory_types::state::Sender;
use factory_types::{AgentKey, CardId};
use serde_json::{Value, json};

use super::Engine;
use crate::jobs::preview::{self as worker, Worker};
use crate::{clock, random};

/// The workers running, and those left down: stopped, or exited on their own.
#[derive(Default)]
pub struct Previews {
    running: BTreeMap<CardId, Worker>,
    down: BTreeSet<CardId>,
}

impl Engine {
    /// Ends the workers of cards that serve none now, marks down those that exited, and starts
    /// the rest of those that should run.
    pub(super) fn previews_tick(&mut self) {
        let wanted: BTreeMap<CardId, (String, u16)> = preview::wanted(&self.state, &self.ctx.config)
            .into_iter()
            .map(|(id, cmd, port)| (id, (cmd.to_owned(), port)))
            .collect();
        let gone = self.previews.running.keys().chain(&self.previews.down).filter(|id| !wanted.contains_key(*id));
        let gone: Vec<CardId> = gone.cloned().collect();
        for id in gone {
            self.previews.running.remove(&id);
            self.previews.down.remove(&id);
        }
        let exited: Vec<(CardId, String)> =
            self.previews.running.iter_mut().filter_map(|(id, w)| w.exited().map(|s| (id.clone(), s))).collect();
        for (id, status) in exited {
            self.previews.running.remove(&id);
            self.previews.down.insert(id.clone());
            self.log(&format!("{id}'s preview exited ({status}): it stays down until started again"));
        }
        for (id, (command, port)) in wanted {
            if self.previews.running.contains_key(&id) || self.previews.down.contains(&id) {
                continue;
            }
            if let Err(e) = self.preview_start(&id, &command, port) {
                self.previews.down.insert(id.clone());
                self.log(&format!("{id}'s preview didn't start: {e}"));
            }
        }
    }

    /// Ends every worker: the app stops or execs another build, whose start starts them again.
    pub(super) fn previews_end(&mut self) {
        self.previews.running.clear();
    }

    /// `preview <card> [action]`, or `demo <card>` (`demo`): Andrei's call decided, the worker
    /// started, restarted or stopped, and only then his call recorded.
    pub(super) fn preview_cmd(
        &mut self,
        card: CardId,
        action: Option<String>,
        demo: bool,
        from: Sender,
    ) -> Result<Value, ErrorBody> {
        let refused = |e: String| ErrorBody::new(ErrorCode::Refused, e);
        let cmd = if demo {
            Command::Demo { card: card.clone() }
        } else {
            Command::Preview { card: card.clone(), action: action.clone() }
        };
        let name = cmd.name();
        let now = clock::now_ms();
        let d = self.decide(&Event::Command { from, cmd }, now);
        // A refusal, the buzz's among them, is committed with its journal before any worker moves.
        if !matches!(d.answer, Some(Ok(_))) {
            return self.commit_decided(d, now, None).answer.unwrap_or_else(|| Ok(json!({})));
        }
        let action = Action::parse(action.as_deref()).map_err(|e| ErrorBody::new(ErrorCode::Invalid, e))?;
        let id = CardId::new(card.as_str().to_lowercase());
        let (command, port) = self
            .state
            .cards
            .get(&id)
            .and_then(|c| preview::serves(c, &self.ctx.config))
            .map(|(c, p)| (c.to_owned(), p))
            .ok_or_else(|| refused(format!("{id} serves no preview")))?;
        if matches!(action, Action::Stop | Action::Restart) {
            self.previews.running.remove(&id);
        }
        // A worker that doesn't start leaves his call unrecorded: no flag, announcement or request (fac-222).
        if action != Action::Stop
            && !self.previews.running.contains_key(&id)
            && let Err(e) = self.preview_start(&id, &command, port)
        {
            self.previews.down.insert(id.clone());
            return Err(refused(e));
        }
        let mut answer = self.commit_decided(d, now, None).answer.unwrap_or_else(|| Ok(json!({})))?;
        let running = self.previews.running.contains_key(&id);
        if let Some(body) = answer.get_mut(&name).and_then(Value::as_object_mut) {
            body.insert("running".into(), json!(running));
        }
        Ok(answer)
    }

    // Its clone read-write, any port, the project's toolchains on its path, and its card's variables.
    fn preview_start(&mut self, id: &CardId, command: &str, port: u16) -> Result<(), String> {
        let c = self.card(id)?;
        let clone = self.clone_dir(id.as_str());
        // A link in its place could take the worker's log elsewhere (R9).
        if !std::fs::symlink_metadata(clone.join(".git")).is_ok_and(|m| m.is_dir()) {
            return Err(format!("{id} has no clone: its `.git` isn't a folder"));
        }
        let owner = AgentKey::new(format!("{id}/preview"));
        let now = clock::now_ms();
        let job = factory_core::state::names::job_id(now, &owner, random::salt());
        let toolchains = self.ctx.project_toolchains(Some(c.project.as_str()));
        let mut env = self.ctx.env(&job, &owner, None, &toolchains, &self.opts.home.job(job.as_str()).join("io"));
        // No turn's: the agents' socket knows no job of it.
        for k in ["FACTORY_JOB", "FACTORY_AGENT", "FACTORY_AGENT_SOCK"] {
            env.remove(k);
        }
        env.insert("FACTORY_CARD".into(), id.as_str().to_owned());
        env.insert("FACTORY_PROJECT".into(), c.project.to_string());
        env.insert("FACTORY_PORT".into(), port.to_string());
        let places = self.ctx.places(&clone, Vec::new(), Vec::new(), None, &toolchains)?;
        let cwd = clone.display().to_string();
        let argv = vec!["sh".into(), "-c".into(), command.to_owned()];
        let spec = self.ctx.spec(&job, factory_types::job::JobKind::Check, &owner, argv, &cwd, &places, env, None, now);
        let worker =
            worker::start(self.opts.home.root(), &spec, "preview", preview::LOG, command).map_err(|e| e.to_string())?;
        self.previews.down.remove(id);
        self.previews.running.insert(id.clone(), worker);
        Ok(())
    }
}
