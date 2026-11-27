// 1.12.8, 1.10.8
#![forbid(unsafe_code)]

mod checks;

use std::process::{Command, ExitCode};

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(xtask::root());
    c
}

// A step that fails says why, even when its child said nothing or never started (fac-141).
fn run(mut c: Command, what: &str) -> bool {
    eprintln!("xtask: {what}");
    let cause = match c.status() {
        Ok(s) if s.success() => return true,
        Ok(s) => s.to_string(),
        Err(e) => format!("couldn't run {}: {e}", c.get_program().display()),
    };
    eprintln!("xtask: {what}: FAILED, {cause}");
    false
}

// The last line names every step's verdict: the train keeps only the output's tail (fac-141).
fn verdict(task: &str, steps: &[(&str, bool)]) -> bool {
    let said: Vec<String> = steps.iter().map(|(s, ok)| format!("{s} {}", if *ok { "ok" } else { "FAILED" })).collect();
    eprintln!("xtask: {task}: {}", said.join(", "));
    steps.iter().all(|(_, ok)| *ok)
}

fn audits() -> Vec<String> {
    let root = xtask::root();
    let mut problems = Vec::new();
    match xtask::graph::metadata(&root) {
        Ok(meta) => {
            problems.extend(xtask::graph::check_graph(&meta));
            problems.extend(xtask::graph::check_forbid(&meta, &root));
        }
        Err(e) => problems.push(e),
    }
    problems.extend(xtask::source::check(&root));
    problems.extend(xtask::caps::check(&root));
    problems.extend(xtask::sentences::check(&root));
    problems
}

fn report(problems: &[String]) -> bool {
    for p in problems {
        eprintln!("  {p}");
    }
    problems.is_empty()
}

fn comments() -> bool {
    eprintln!("xtask: comments");
    report(&xtask::comments::check(&xtask::root()))
}

fn web_classes(web: &std::path::Path) -> bool {
    eprintln!("xtask: web classes");
    let found = match xtask::web::check_classes(web) {
        Ok(found) => found,
        Err(e) => return report(&[e]),
    };
    let lines: Vec<String> = found.iter().map(|e| format!("web/{}:{}: {}", e.file, e.line, e.summary)).collect();
    report(&lines)
}

fn web() -> bool {
    let dir = xtask::root().join("web");
    if xtask::web::needs_install(&dir) && !run(xtask::web::npm(&dir, &["ci"]), "web install") {
        return false;
    }
    web_classes(&dir)
        && run(xtask::web::npm(&dir, &["run", "typecheck"]), "web typecheck")
        && run(xtask::web::npm(&dir, &["run", "build"]), "web build")
}

fn rustfmt() -> Command {
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--check"]);
    fmt
}

const CLIPPY_PASSES: [(&str, &[&str]); 2] =
    [("clippy", &["--workspace", "--all-targets", "--all-features"]), ("production clippy", &xtask::suite::PRODUCTION)];

fn clippy(targets: &[&str], json: bool) -> Command {
    let mut cmd = cargo();
    cmd.arg("clippy").args(targets);
    if json {
        cmd.arg("--message-format=json");
    }
    cmd.args(["--", "-D", "warnings"]);
    cmd
}

fn check() -> bool {
    let wire_ok = report(&xtask::web_types::check(&xtask::root()).err().into_iter().collect::<Vec<_>>());
    let fmt_ok = run(rustfmt(), "rustfmt");
    let comments_ok = comments();
    let mut steps = vec![("web types", wire_ok), ("rustfmt", fmt_ok), ("comments", comments_ok)];
    // Clippy has no lint for a comment's length: the comment rules fail the check before it runs (fac-2).
    if !comments_ok {
        return verdict("check", &steps);
    }
    for (name, targets) in CLIPPY_PASSES {
        steps.push((name, run(clippy(targets, false), name)));
    }
    eprintln!("xtask: audits");
    steps.push(("audits", report(&audits())));
    steps.push(("web", web()));
    verdict("check", &steps)
}

const FAULTS: [&str; 2] = ["--features", "mork-app/faults,mork-engine/faults"];

// The merge train runs the suite walled, then its recorded wall tests unwalled (fac-65). With no phase,
// as an older engine's train runs it, it runs once as before; fac-123 refuses that after fac-65's reload.
fn test() -> bool {
    match xtask::walls::phase() {
        Ok(Some(xtask::walls::Phase::Walled)) => suite(true),
        Ok(Some(xtask::walls::Phase::Unwalled)) => walls(),
        Ok(None) => suite(false),
        Err(e) => report(&[e]),
    }
}

// Each pass builds right before it runs: both leave their `mork` at the one path the tests run. Only
// the run is timed. The production build proves `faults` compiles out, and runs the tests named `production`.
fn suite(walled: bool) -> bool {
    let root = xtask::root();
    let records = walled.then(|| xtask::walls::records(&root));
    if let Some(Err(e)) = records.as_deref().map(xtask::walls::fresh) {
        return report(&[e]);
    }
    eprintln!("xtask: building the suite, with faults");
    let faults = recording(xtask::suite::build(&root, &xtask::suite::WORKSPACE, &FAULTS, &[]), records.as_deref());
    let fakes = run(fakes(), "building the fakes");
    let start = std::time::Instant::now();
    let a = pass(faults, "the suite, with faults", None);
    let mut doc = cargo();
    doc.args(["test", "--no-fail-fast", "--workspace", "--doc"]).args(FAULTS);
    let d = run(doc, "the documentation tests, with faults");
    let elapsed = start.elapsed();
    eprintln!("xtask: the suite ran in {}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60);
    let timed = report(&xtask::budget::within(elapsed).err().into_iter().collect::<Vec<_>>());
    eprintln!("xtask: building the suite, production-only");
    let production = xtask::suite::build(&root, &xtask::suite::PRODUCTION, &[], &["production"]);
    let b = pass(recording(production, records.as_deref()), "the production-only tests", None);
    let c = hygiene();
    let steps = [
        ("the fakes", fakes),
        ("the suite, with faults", a),
        ("the documentation tests", d),
        ("the time budget", timed),
        ("the production-only tests", b),
        ("hygiene", c),
    ];
    verdict("test", &steps)
}

// The walled phase's binaries record their skipped wall tests; with no phase none does.
fn recording(
    binaries: Result<Vec<xtask::suite::Binary>, String>,
    records: Option<&std::path::Path>,
) -> Result<Vec<xtask::suite::Binary>, String> {
    let mut binaries = binaries?;
    if let Some(dir) = records {
        xtask::walls::record(&mut binaries, dir);
    }
    Ok(binaries)
}

// Both builds' recorded wall tests, outside every wall; none at all means nothing proved the walls. Only the
// passes are timed, against their own budget (fac-170).
fn walls() -> bool {
    let root = xtask::root();
    let records = xtask::walls::records(&root);
    let fakes = run(fakes(), "building the fakes");
    let mut steps = vec![("the fakes", fakes)];
    let mut ran = 0;
    let mut elapsed = std::time::Duration::ZERO;
    let builds: [(&[&str], &[&str], &str); 2] = [
        (&xtask::suite::WORKSPACE, &FAULTS, "the wall tests, with faults"),
        (&xtask::suite::PRODUCTION, &[], "the production-only wall tests"),
    ];
    for (select, extra, what) in builds {
        eprintln!("xtask: building {what}");
        let picked = xtask::suite::build(&root, select, extra, &[]).and_then(|b| xtask::walls::rerun(b, &records));
        let named = picked.as_ref().map_or(0, |b| xtask::walls::named(b));
        eprintln!("xtask: {what}: {named} recorded");
        ran += named;
        let start = std::time::Instant::now();
        steps.push((what, pass(picked, what, Some(named))));
        elapsed += start.elapsed();
    }
    eprintln!("xtask: the unwalled passes ran in {}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60);
    steps
        .push(("the time budget", report(&xtask::budget::walls_within(elapsed).err().into_iter().collect::<Vec<_>>())));
    if ran == 0 {
        let none = report(&["the walled phase recorded no wall test to run outside the wall".to_owned()]);
        steps.push(("a recorded wall test", none));
    }
    steps.push(("hygiene", hygiene()));
    verdict("test", &steps)
}

// The fakes each test binary builds on its first test home, built once before the binaries run at once.
fn fakes() -> Command {
    let mut c = cargo();
    c.args(["build", "--quiet", "--bins", "--manifest-path", "crates/fakes/Cargo.toml"]);
    c
}

fn pass(binaries: Result<Vec<xtask::suite::Binary>, String>, what: &str, known_count: Option<usize>) -> bool {
    let binaries = match binaries {
        Ok(b) => b,
        Err(e) => return report(&[format!("{what}: {e}")]),
    };
    let at_most = xtask::suite::at_most();
    // The engine reads the count as the suite's total, adding each pass's as it starts (fac-697).
    let count = known_count.or_else(|| xtask::suite::count(&binaries));
    let tests = count.map_or_else(String::new, |n| format!(" {n} tests,"));
    eprintln!("xtask: {what}: {} test binaries,{tests} {at_most} at a time", binaries.len());
    let failed = xtask::suite::run_all(&binaries, at_most);
    if !failed.is_empty() {
        eprintln!("xtask: {what}: {} of {} test binaries failed", failed.len(), binaries.len());
    }
    report(&failed)
}

fn hygiene() -> bool {
    eprintln!("xtask: hygiene");
    match xtask::hygiene::homes_dir(&xtask::root()) {
        Ok(dir) => report(&xtask::hygiene::check(&dir)),
        Err(e) => report(&[e]),
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("checks") {
        return ExitCode::from(checks::run());
    }
    let ok = match std::env::args().nth(1).as_deref() {
        Some("check") => check(),
        Some("web") => web(),
        Some("web-types") => report(&xtask::web_types::write(&xtask::root()).err().into_iter().collect::<Vec<_>>()),
        Some("audit") => {
            let a = comments();
            eprintln!("xtask: audits");
            let b = report(&audits());
            verdict("audit", &[("comments", a), ("audits", b)])
        }
        Some("test") => test(),
        Some("hygiene") => hygiene(),
        _ => {
            eprintln!("usage: cargo xtask <check|checks --base <b> --json <report>|web|web-types|audit|test|hygiene>");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
