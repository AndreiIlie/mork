//! A live turn's stdin (fac-389): its prompt, then each message delivered while it works, spoken as its CLI reads
//! them. Claude stages each with `shouldQuery: false` for its next call; Codex's app-server steers its turn.

use mork_types::config::Effort;
use mork_types::job::Live;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A message delivered into a running turn: `input/<seq>.json` in its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    pub uuid: String,
    pub text: String,
    /// The images' paths, as the turn sees them.
    pub images: Vec<String>,
}

/// Message `seq`'s id in job `job`'s stream: a UUID, as the CLIs take, whose first 24 characters only that job's
/// messages share.
pub fn message_uuid(job: &str, seq: u64) -> String {
    let h = job.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let (a, b, c, d) = (h >> 32, (h >> 16) & 0xffff, (h >> 4) & 0xfff, h & 0xfff);
    format!("{a:08x}-{b:04x}-4{c:03x}-8{d:03x}-{:012x}", seq & 0xffff_ffff_ffff)
}

/// The seq of one of job `job`'s message ids; none for any other id, as the prompt's own.
pub fn seq_of(job: &str, uuid: &str) -> Option<u64> {
    let own = message_uuid(job, 0);
    let (prefix, tail) = (uuid.get(..24)?, uuid.get(24..)?);
    if prefix != &own[..24] || tail.len() != 12 {
        return None;
    }
    u64::from_str_radix(tail, 16).ok()
}

/// Drives a live turn's stdin until Claude's result or Codex's turn end.
#[derive(Debug)]
pub enum Driver {
    Claude { session: String, done: bool },
    Codex(Codex),
}

/// Opens the thread and turn before sending the messages waiting for it.
#[derive(Debug)]
pub struct Codex {
    prompt: String,
    images: Vec<String>,
    schema: Option<Value>,
    thread: Option<String>,
    turn: Option<String>,
    waiting: Vec<Delivery>,
    next_id: u64,
    done: bool,
    model: Option<String>,
    effort: Option<Effort>,
}

const THREAD_ID: u64 = 2;
const TURN_ID: u64 = 3;
// Steers are numbered from here, apart from the requests that start the turn.
const FIRST_STEER: u64 = 100;

impl Driver {
    /// Returns the driver and its opening lines. Claude's schema is already on its command line.
    pub fn start(live: &Live, prompt: &str, schema: Option<Value>) -> (Self, Vec<String>) {
        match live {
            Live::Claude { session } => {
                let first = json!({"type": "user", "session_id": session, "parent_tool_use_id": null,
                    "message": {"role": "user", "content": prompt}});
                (Self::Claude { session: session.clone(), done: false }, vec![first.to_string()])
            }
            Live::Codex { thread, images, model, effort } => {
                let codex = Codex {
                    prompt: prompt.to_owned(),
                    images: images.clone(),
                    schema,
                    thread: None,
                    turn: None,
                    waiting: Vec::new(),
                    next_id: FIRST_STEER,
                    done: false,
                    model: model.clone(),
                    effort: *effort,
                };
                let lines = codex.start(thread.as_deref());
                (Self::Codex(codex), lines)
            }
        }
    }

    /// One complete line of the turn's stream: what it answers.
    pub fn output(&mut self, line: &str) -> Vec<String> {
        let Ok(ev) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match self {
            Self::Claude { done, .. } => {
                // Nothing staged after the first result is read by this turn.
                if ev.get("type").and_then(Value::as_str) == Some("result") {
                    *done = true;
                }
                Vec::new()
            }
            Self::Codex(c) => c.output(&ev),
        }
    }

    /// A message delivered: the lines that hand it to the turn, now or once its turn is known.
    pub fn deliver(&mut self, d: Delivery) -> Vec<String> {
        match self {
            Self::Claude { done: true, .. } => Vec::new(),
            Self::Claude { session, .. } => {
                // Staged for the model's next call, as a person's words: no turn of its own (fac-389).
                let line = json!({"type": "user", "uuid": d.uuid, "session_id": session, "parent_tool_use_id": null,
                    "client_composed": true, "origin": {"kind": "human"}, "shouldQuery": false, "priority": "next",
                    "message": {"role": "user", "content": d.text}});
                vec![line.to_string()]
            }
            Self::Codex(c) if c.done => Vec::new(),
            Self::Codex(c) => {
                c.waiting.push(d);
                c.flush()
            }
        }
    }

    /// Nothing more is written: its stdin is closed.
    pub fn done(&self) -> bool {
        match self {
            Self::Claude { done, .. } => *done,
            Self::Codex(c) => c.done,
        }
    }
}

impl Codex {
    // The initialize handshake, then the thread: `thread` resumed on this job's selection, whose metadata's model and
    // effort would otherwise win over the command line's (fac-637), else a new one in the process's folder.
    fn start(&self, thread: Option<&str>) -> Vec<String> {
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "mork", "title": "mork", "version": "1"},
            "capabilities": {"experimentalApi": false}}});
        let initialized = json!({"jsonrpc": "2.0", "method": "initialized"});
        let open = match thread {
            Some(t) => {
                let mut params = json!({"threadId": t});
                if let Some(model) = &self.model {
                    params["model"] = json!(model);
                }
                if let Some(effort) = self.effort {
                    params["config"] = json!({"model_reasoning_effort": effort.as_str()});
                }
                json!({"jsonrpc": "2.0", "id": THREAD_ID, "method": "thread/resume", "params": params})
            }
            None => json!({"jsonrpc": "2.0", "id": THREAD_ID, "method": "thread/start", "params": {}}),
        };
        vec![init.to_string(), initialized.to_string(), open.to_string()]
    }

    fn output(&mut self, ev: &Value) -> Vec<String> {
        if self.done {
            return Vec::new();
        }
        let id = ev.get("id");
        if let Some(method) = ev.get("method").and_then(Value::as_str) {
            // A request of the server's waits on an answer nobody here gives but this refusal.
            if let Some(id) = id {
                let no = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not supported"}});
                return vec![no.to_string()];
            }
            // A child's completion must leave the parent's stdin open (fac-526).
            if let (Some(root), Some(thread)) =
                (self.thread.as_deref(), ev.pointer("/params/threadId").and_then(Value::as_str))
                && root != thread
            {
                return Vec::new();
            }
            match method {
                "turn/started" => return self.turn_known(ev.pointer("/params/turn/id")),
                "turn/completed" => self.done = true,
                _ => {}
            }
            return Vec::new();
        }
        match id.and_then(Value::as_u64) {
            Some(THREAD_ID) => match ev.pointer("/result/thread/id").and_then(Value::as_str) {
                Some(t) => {
                    self.thread = Some(t.to_owned());
                    vec![self.turn_start(t)]
                }
                None => {
                    self.done = true;
                    Vec::new()
                }
            },
            Some(TURN_ID) if ev.get("error").is_some() => {
                self.done = true;
                Vec::new()
            }
            Some(TURN_ID) => self.turn_known(ev.pointer("/result/turn/id")),
            _ => Vec::new(),
        }
    }

    fn turn_start(&self, thread: &str) -> String {
        let mut input = vec![json!({"type": "text", "text": self.prompt})];
        input.extend(self.images.iter().map(|p| json!({"type": "localImage", "path": p})));
        let mut params = json!({"threadId": thread, "input": input});
        if let Some(schema) = &self.schema {
            params["outputSchema"] = schema.clone();
        }
        if let Some(model) = &self.model {
            params["model"] = json!(model);
        }
        if let Some(effort) = self.effort {
            params["effort"] = json!(effort.as_str());
        }
        json!({"jsonrpc": "2.0", "id": TURN_ID, "method": "turn/start", "params": params}).to_string()
    }

    fn turn_known(&mut self, turn: Option<&Value>) -> Vec<String> {
        if self.turn.is_none() {
            self.turn = turn.and_then(Value::as_str).map(str::to_owned);
        }
        self.flush()
    }

    // Each waiting message steers the turn, its id the message's, so the stream says when the model took it.
    fn flush(&mut self) -> Vec<String> {
        let (Some(thread), Some(turn)) = (self.thread.clone(), self.turn.clone()) else { return Vec::new() };
        let mut lines = Vec::new();
        for d in std::mem::take(&mut self.waiting) {
            let mut input = vec![json!({"type": "text", "text": d.text})];
            input.extend(d.images.iter().map(|p| json!({"type": "localImage", "path": p})));
            let params =
                json!({"threadId": thread, "expectedTurnId": turn, "input": input, "clientUserMessageId": d.uuid});
            lines.push(
                json!({"jsonrpc": "2.0", "id": self.next_id, "method": "turn/steer", "params": params}).to_string(),
            );
            self.next_id += 1;
        }
        lines
    }
}
