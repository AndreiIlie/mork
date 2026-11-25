//! Scenarios (1.10.2): `FAKE_SCENARIO` says what each run of a fake does, matched by its `FACTORY_AGENT`, run number
//! or a phrase in its prompt; the first run entry whose given keys all match is played, else the fake's default.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "do", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Step {
    /// Claude's `system/init`, or agy's `init`, with the session id or conversation from its argv.
    Init,
    /// A result whose `structured_output` is `value` (`{io}`, `{job}`, `{jobs}` and `{seats}` in it are this
    /// turn's `$FACTORY_IO`, its job's folder, the jobs' folder and the seats' `io/` folders).
    Reply {
        value: Value,
        /// The session's cost total it reports (`total_cost_usd`), 0.01 when not given.
        #[serde(default)]
        cost: Option<f64>,
    },
    /// A result whose `structured_output` is `{"message": text}`: the echo role's reply.
    Echo {
        text: String,
    },
    /// Codex's `thread.started`, with the thread id from argv or a new one; an app-server turn's `thread/started`
    /// and `turn/started`, its thread the one it opened.
    ThreadStarted {
        thread: Option<String>,
    },
    /// Codex: a completed turn, writing `-o`'s last message as `value`; an app-server turn's last agent message
    /// (`{io}` and the others in it as in `Reply`).
    CodexReply {
        value: Value,
    },
    Sleep {
        ms: u64,
    },
    /// Sleeps until killed.
    Hang,
    Exit {
        code: i32,
    },
    /// Writes a file (relative to the working folder, or under `{io}`).
    WriteFile {
        path: String,
        text: String,
    },
    /// Runs a shell script in its folder, as the agent would (git, in the wall), `{io}` and the
    /// others as `Reply`'s.
    Sh {
        script: String,
    },
    /// Makes `link` a symlink to `target` (`{io}` and the others in `link` as in `Reply`).
    Symlink {
        link: String,
        target: String,
    },
    /// Runs the probe with this spec, and prints its line.
    Probe {
        spec: Value,
    },
    /// Runs `factory ctl` with these arguments (`{io}` and the others in them as in `Reply`), and prints
    /// `{"type":"fake-ctl",…}`.
    Ctl {
        args: Vec<String>,
    },
    /// `cargo build` makes `<--target-dir>/release/factory`, a script running this build as `FACTORY_BUILD_COMMIT`
    /// (1.10.6) or `commit`; with `link`, a symlink to that path instead.
    CargoBuild {
        #[serde(default)]
        commit: Option<String>,
        #[serde(default)]
        link: Option<String>,
    },
    /// Appends the run's environment and folder to a JSON line (`{"type":"fake-env",…}`).
    ShowEnv,
    /// The checks tool's answer (F-H1): writes `report` (its `head` filled in) to the `--json`
    /// file when given, then exits with `exit`.
    Checks {
        exit: i32,
        report: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// The fake's name (`claude`, `codex`, `cargo`…); any when absent.
    pub fake: Option<String>,
    pub agent: Option<String>,
    /// The run number of this fake, from 1.
    pub run: Option<u64>,
    pub phrase: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default)]
    pub runs: Vec<Run>,
}

/// What the run is matched on.
#[derive(Debug, Clone)]
pub struct Who<'a> {
    pub fake: &'a str,
    pub agent: &'a str,
    pub run: u64,
    pub prompt: &'a str,
}

impl Scenario {
    /// Reads `FAKE_SCENARIO`; an empty scenario when unset or missing.
    pub fn load() -> Self {
        let Some(path) = std::env::var_os("FAKE_SCENARIO") else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| panic!("bad scenario {}: {e}", path.display())),
            Err(_) => Self::default(),
        }
    }

    /// The steps of the first matching run.
    pub fn steps_for(&self, who: &Who<'_>) -> Option<Vec<Step>> {
        self.runs
            .iter()
            .find(|r| {
                r.fake.as_deref().is_none_or(|f| f == who.fake)
                    && r.agent.as_deref().is_none_or(|a| a == who.agent)
                    && r.run.is_none_or(|n| n == who.run)
                    && r.phrase.as_deref().is_none_or(|p| who.prompt.contains(p))
            })
            .map(|r| r.steps.clone())
    }
}

/// The default run of a CLI fake: start, then reply as the echo role does.
pub fn default_steps(fake: &str) -> Vec<Step> {
    match fake {
        "codex" => vec![
            Step::ThreadStarted { thread: None },
            Step::CodexReply { value: serde_json::json!({"message": "echo"}) },
        ],
        "claude" | "agy" => vec![Step::Init, Step::Echo { text: "echo".into() }],
        "cargo" => vec![Step::CargoBuild { commit: None, link: None }],
        "fake-checks" => vec![Step::Checks { exit: 0, report: Some(serde_json::json!({"status": 0, "findings": []})) }],
        _ => vec![Step::Exit { code: 0 }],
    }
}
