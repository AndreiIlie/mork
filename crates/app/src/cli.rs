// 1.7

use std::path::{Path, PathBuf};

use mork_engine::cli::claude_hooks::Hook;
use mork_engine::paths::Home;
use mork_engine::runtime::{self, Options};
use mork_engine::{clock, config, faults, jobs};

use crate::{ctl, launcher, local};

/// The commit this binary reports: `MORK_TEST_COMMIT` in a test build, else its own.
pub fn commit() -> String {
    faults::test_env("MORK_TEST_COMMIT").unwrap_or_else(|| env!("MORK_COMMIT").to_owned())
}

/// Whether this binary is of a tree with changes (`hello.dirty`).
pub fn dirty() -> bool {
    env!("MORK_DIRTY") == "true"
}

pub fn bin() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("mork"))
}

fn home() -> Result<Home, i32> {
    Home::from_env().ok_or_else(|| {
        eprintln!("mork: no home: set MORK_HOME, XDG_DATA_HOME or HOME");
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
            None => usage("mork supervise <job folder>"),
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
                    eprintln!("mork work: {e}");
                    1
                }
            },
            None => usage("mork work <order.json>"),
        },
        Some("test") => jobs::slot_client::run(rest, &commit()),
        Some("manual") => manual(rest),
        Some("claude-agent-hook") => claude_hook(Hook::Agent { fable: rest.iter().any(|arg| arg == "--seat-fable") }),
        Some("claude-path-hook") => claude_hook(Hook::Path),
        // The door's dispatcher (fac-1, fac-86): the engine's own local child, reaching a target
        // over the operator's ssh alias with the box's own tools, nothing of Mork's installed there.
        Some("door-dispatch") => jobs::door_dispatch::main(rest),
        Some("live-probe") => mork_app::live::probe(rest),
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
            None => usage("mork --reloaded <id>"),
        },
        Some("ctl") => ctl::run(&home, &commit(), &args[1..]),
        Some("build") => local::build(&home, rest),
        Some("pin") => local::pin(&home, rest),
        Some("status") => local::status(&home),
        Some("doctor") => local::doctor(&home),
        Some("login") => local::login(&home, rest),
        Some("install") => local::install(rest),
        Some("web") => local::web(&home),
        Some("live-check") => mork_app::live::run(&home, &bin()),
        Some(other) => usage(&format!("mork: unknown command `{other}`; see 1.7")),
    }
}

fn usage(text: &str) -> i32 {
    eprintln!("{text}");
    2
}

fn claude_hook(hook: Hook) -> i32 {
    match hook.check(std::io::stdin().lock()) {
        Ok(()) => 0,
        Err(reason) => {
            eprintln!("{reason}");
            2
        }
    }
}

// The running build's own text, built in: inside a wall `mork` is that build (fac-14).
fn manual(rest: &[String]) -> i32 {
    let names = mork_core::maintenance::manuals::names().join(", ");
    match rest.first().map(String::as_str) {
        None => {
            println!("mork manual <name>: {names}");
            0
        }
        Some(name) => match mork_core::maintenance::manuals::manual(name) {
            Some(text) => {
                print!("{text}");
                0
            }
            None => usage(&format!("mork manual: no manual `{name}`; the manuals are {names}")),
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
    let Some(config_path) = config::path() else { return usage("mork: no config path: set MORK_CONFIG or HOME") };
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
    let settings = config::read(&config_path)
        .map_or(mork_ui::app::Settings { editor: String::new(), animations: true, tunnels: Default::default() }, |c| {
            mork_ui::app::Settings { editor: c.mork.editor, animations: c.mork.animations, tunnels: c.tunnels }
        });
    crate::window::run(home, settings, move || runtime::run(opts))
}
