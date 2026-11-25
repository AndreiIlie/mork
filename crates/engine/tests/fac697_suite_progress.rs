// fac-697: a running suite's counts, read from its output a line at a time.
// why: test code: a failed unwrap is the test failing, which 1.12.4 leaves to tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use factory_engine::jobs::suite_progress::{Output, Reader};
use factory_types::view::SuiteProgress;

fn read(output: &str) -> Option<SuiteProgress> {
    let mut r = Reader::default();
    for line in output.lines() {
        r.line(line);
    }
    r.progress()
}

#[test]
fn xtask_passes_add_their_counts_and_its_binaries_output_is_not_counted() {
    let output = "\
xtask: building the suite, with faults
xtask: the suite, with faults: 2 test binaries, 5 tests, 2 at a time
xtask: started factory-core tests/a.rs
xtask: factory-core tests/a.rs: test one ... ok
xtask: factory-core tests/a.rs: test two ... FAILED
xtask: factory-core tests/a.rs: test three ... ignored, needs a wall
xtask: factory-core tests/a.rs: FAILED, exit status: 101, in 0.3s

running 3 tests
test one ... ok
test two ... FAILED
test three ... ignored, needs a wall
xtask: building the suite, production-only
xtask: the production-only tests: 1 test binaries, 1 tests, 2 at a time
xtask: factory-app tests/m0_walled_turn.rs: test m0_production_build_refuses_a_partial_wall ... ok
";
    let want = SuiteProgress {
        passed: 3,
        failed: 1,
        total: Some(6),
        files: false,
        latest: Some("m0_production_build_refuses_a_partial_wall".into()),
    };
    assert_eq!(read(output), Some(want));
}

#[test]
fn an_xtask_pass_without_its_count_leaves_the_whole_total_unknown() {
    let output = "\
xtask: the suite, with faults: 1 test binaries, 3 tests, 2 at a time
xtask: factory-core tests/a.rs: test one ... ok
xtask: the production-only tests: 1 test binaries, 2 at a time
";
    let got = read(output).unwrap();
    assert_eq!((got.passed, got.total), (1, None));
}

#[test]
fn jest_counts_test_files_once_and_gives_no_total() {
    let output = "\
> my-app@1.0.0 test:unit
PASS server/features/bills/bills-rules.test.js (5.2 s)
FAIL server/features/payees/payees-service.test.js
  ● payees › refuses a blank IBAN
PASS  unit  app/shared/money.test.js
Summary of all failing tests
FAIL server/features/payees/payees-service.test.js
Test Suites: 1 failed, 2 passed, 3 total
";
    let want = SuiteProgress {
        passed: 2,
        failed: 1,
        total: None,
        files: true,
        latest: Some("app/shared/money.test.js".into()),
    };
    assert_eq!(read(output), Some(want));
}

#[test]
fn output_no_reader_knows_gives_no_progress() {
    assert_eq!(read("Compiling foo v0.1.0\nrunning 3 tests\ntest a ... ok\n"), None);
    assert_eq!(read("xtask: building the suite, with faults\n"), None);
}

#[test]
fn a_suite_output_made_anew_after_the_install_is_read_from_its_start() {
    let dir = std::env::temp_dir().join(format!("factory-test-suite-progress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("suite.out");
    std::fs::write(&file, "added 1200 packages, and audited 1201 packages in 9s\nfound 0 vulnerabilities\n").unwrap();
    let mut out = Output::default();
    out.more(&file);
    assert_eq!(out.progress(), None);
    std::fs::remove_file(&file).unwrap();
    std::fs::write(&file, "PASS a.test.js\nFAIL b.test.js\n").unwrap();
    out.more(&file);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(out.progress().map(|p| (p.passed, p.failed)), Some((1, 1)));
}
