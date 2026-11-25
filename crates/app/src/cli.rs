// 1.7

use std::path::{Path, PathBuf};

use factory_engine::paths::Home;
use factory_engine::runtime::{self, Options};
use factory_engine::{clock, config, faults, jobs};

use crate::{ctl, launcher, local};

/// The commit this binary reports: `FACTORY_TEST_COMMIT` in a test build, else its own.
pub fn commit() -> String {
    faults::test_env("FACTORY_TEST_COMMIT").unwrap_or_else(|| env!("FACTORY_COMMIT").to_owned())
}

/// Whether this binary is of a tree with changes (`hello.dirty`).
pub fn dirty() -> bool {
    env!("FACTORY_DIRTY") == "true"
}

pub fn bin() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("factory"))
}

fn home() -> Result<Home, i32> {
    Home::from_env().ok_or_else(|| {
        eprintln!("factory: no home: set FACTORY_HOME, XDG_DATA_HOME or HOME");
        2
    })
}

/// Runs the command line; returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let rest = args.get(2..).unwrap_or(&[]);
    let sub = args.get(1).map(String::as_str);
    match sub {
        Some("supervise") => match rest.first() {
            Some(dir) => jobs::supervisor::main(Path::new(dir)),
            None => usage("factory supervise <job folder>"),
        },
        Some("wall-exec") => {
            eprintln!("{}", jobs::wall_exec::run(rest));
            126
        }
        Some("version") => local::version(rest),
        Some("work") => match rest.first() {
            Some(order) => match jobs::work::run(Path::new(order)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("factory work: {e}");
                    1
                }
            },
            None => usage("factory work <order.json>"),
        },
        Some("test") => jobs::slot_client::run(rest, &commit()),
        Some("manual") => manual(rest),
        Some("claude-agent-hook") => claude_agent_hook(rest),
        // The door's dispatcher (fac-1, fac-86): the engine's own local child, reaching a target
        // over Andrei's ssh alias with the box's own tools, nothing of the Factory's installed there.
        Some("door-dispatch") => jobs::door_dispatch::main(rest),
        Some("live-probe") => factory_app::live::probe(rest),
        _ => with_home(sub, args, rest),
    }
}

fn with_home(sub: Option<&str>, args: &[String], rest: &[String]) -> i32 {
    let home = match home() {
        Ok(h) => h,
        Err(c) => return c,
    };
    match sub {
        None | Some("--no-window") => app(&home, args, None),
        Some("--reloaded") => match rest.first() {
            Some(id) => app(&home, args, Some(id.clone())),
            None => usage("factory --reloaded <id>"),
        },
        Some("ctl") => ctl::run(&home, &commit(), &args[1..]),
        Some("build") => local::build(&home, rest),
        Some("pin") => local::pin(&home, rest),
        Some("status") => local::status(&home),
        Some("doctor") => local::doctor(&home),
        Some("login") => local::login(&home, rest),
        Some("install") => local::install(rest),
        Some("web") => local::web(&home),
        Some("live-check") => factory_app::live::run(&home, &bin()),
        Some(other) => usage(&format!("factory: unknown command `{other}`; see 1.7")),
    }
}

fn usage(text: &str) -> i32 {
    eprintln!("{text}");
    2
}

fn claude_agent_hook(rest: &[String]) -> i32 {
    // Input failures must not stop every subagent (fac-403).
    let Ok(input) = serde_json::from_reader::<_, serde_json::Value>(std::io::stdin().lock()) else { return 0 };
    if input.get("tool_name").and_then(serde_json::Value::as_str) != Some("Agent") {
        return 0;
    }
    let Some(call) = input.get("tool_input").filter(|v| v.is_object()) else { return 0 };
    let refusal = factory_core::agents::subagents::refusal(
        call.get("model").and_then(serde_json::Value::as_str),
        call.get("subagent_type").and_then(serde_json::Value::as_str),
        rest.iter().any(|arg| arg == "--seat-fable"),
    );
    if let Some(text) = refusal {
        eprintln!("{text}");
        2
    } else {
        0
    }
}

// The running build's own text, built in: inside a wall `factory` is that build (fac-14).
fn manual(rest: &[String]) -> i32 {
    let names = factory_core::maintenance::manuals::names().join(", ");
    match rest.first().map(String::as_str) {
        None => {
            println!("factory manual <name>: {names}");
            0
        }
        Some(name) => match factory_core::maintenance::manuals::manual(name) {
            Some(text) => {
                print!("{text}");
                0
            }
            None => usage(&format!("factory manual: no manual `{name}`; the manuals are {names}")),
        },
    }
}

// Without `--reloaded`, the launcher rule runs first (1.7.4).
fn app(home: &Home, args: &[String], reloaded: Option<String>) -> i32 {
    let began = clock::instant();
    if reloaded.is_none()
        && let Some(code) = launcher::hand_over(home, &commit(), args)
    {
        return code;
    }
    let Some(config_path) = config::path() else { return usage("factory: no config path: set FACTORY_CONFIG or HOME") };
    let no_window = args.iter().skip(1).any(|a| a == "--no-window");
    let mode_args = if no_window { vec!["--no-window".to_owned()] } else { Vec::new() };
    let opts = Options {
        home: home.clone(),
        config_path: config_path.clone(),
        bin: bin(),
        commit: commit(),
        dirty: dirty(),
        began,
        reloaded,
        mode_args,
    };
    if no_window {
        return runtime::run(opts);
    }
    // The engine says what is wrong with a bad config.
    let settings = config::read(&config_path).map_or(
        factory_ui::app::Settings { editor: String::new(), animations: true, tunnels: Default::default() },
        |c| factory_ui::app::Settings {
            editor: c.factory.editor,
            animations: c.factory.animations,
            tunnels: c.tunnels,
        },
    );
    crate::window::run(home, settings, move || runtime::run(opts))
}
