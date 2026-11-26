//! The operator's check of the walls and the real CLIs (1.11); any failure exits 1.
//! The CLIs are whatever `claude` and `codex` the toolchains and PATH give, so a test runs it with the fakes.

mod deny;
mod logins;
mod probe;
mod scratch;
mod turns;
mod wall;

pub use probe::run as probe;

use mork_engine::paths::Home;

/// The checks, as they end.
#[derive(Debug, Default)]
pub struct Report {
    failed: usize,
}

impl Report {
    pub fn check(&mut self, name: &str, result: Result<(), String>) {
        match result {
            Ok(()) => println!("ok      {name}"),
            Err(why) => {
                self.failed += 1;
                println!("FAILED  {name}: {why}");
            }
        }
    }

    /// The exit code: 1 when any check failed.
    pub fn code(&self) -> i32 {
        i32::from(self.failed > 0)
    }
}

/// Runs the whole check (1.11); its exit code.
pub fn run(real: &Home, bin: &std::path::Path) -> i32 {
    let mut report = Report::default();
    // SIGINT or SIGTERM: the check stops where it is, and its clean-up still runs (M5.28).
    if let Err(e) = mork_sys::install_term_flag() {
        report.check("the check's signals taken", Err(e.to_string()));
        return report.code();
    }
    let mut s = match scratch::Scratch::new(real, bin) {
        Ok(s) => s,
        Err(why) => {
            report.check("the scratch home, repository and config", Err(why));
            return report.code();
        }
    };
    report.check("the scratch home, repository and config", Ok(()));
    let started = s.start();
    let up = started.is_ok();
    report.check("Mork starts in the scratch home", started);
    if up {
        turns::run(&mut s, &mut report);
        // A failed step may leave a turn hung: the steps after it would wait it out (fac-170).
        if report.code() != 0 {
            s.stop_jobs();
        }
        if !scratch::interrupted() {
            wall::run(&s, real, bin, &mut report);
        }
        if !scratch::interrupted() {
            deny::run(&mut s, &mut report);
        }
    }
    if scratch::interrupted() {
        report.check("the check ran to its end", Err("interrupted (SIGINT or SIGTERM)".into()));
    }
    report.check("the scratch jobs stopped and the scratch folder removed", s.end());
    println!("{}", if report.code() == 0 { "live check passed" } else { "live check FAILED" });
    report.code()
}
