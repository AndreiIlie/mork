// Rule 1.7.4: run the pinned build when it is another one, unless started with `--reloaded`.
// Exception (1.8.6.9): a `pin.next` build reading a state this build can't runs that build.

use mork_engine::builds::{self, PinNext};
use mork_engine::jobs::launch;
use mork_engine::paths::Home;
use mork_engine::{files, store};

/// Hands over to another build when the rule says so; `None` to carry on as this one.
pub fn hand_over(home: &Home, own: &str, args: &[String]) -> Option<i32> {
    let next: Option<PinNext> = files::read_json(&home.pin_next()).ok().flatten();
    if let Some(next) = next {
        if next.target == own {
            return None;
        }
        let version = store::peek_version(&home.state_db()).ok().flatten();
        let mine = store::schema::OLDEST..=store::schema::NEWEST;
        if let Some(v) = version.filter(|v| !mine.contains(v)) {
            let reads =
                builds::manifest(home, &next.target).is_some_and(|m| m.state_oldest <= v && v <= m.state_newest);
            if reads {
                return Some(exec(home, &next.target, args));
            }
        }
        // A stale pin.next of another build: the pin rule still holds (1.8.6.8); the start
        // that runs says the reload failed and removes it.
    }
    let pin = builds::pin(home)?;
    if pin != own && builds::binary(home, &pin).exists() {
        return Some(exec(home, &pin, args));
    }
    None
}

fn exec(home: &Home, commit: &str, args: &[String]) -> i32 {
    let err = launch::exec(&builds::binary(home, commit), args.get(1..).unwrap_or(&[]), &[]);
    eprintln!("mork: couldn't run build {commit}: {err}");
    1
}
