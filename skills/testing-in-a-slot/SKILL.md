---
name: testing-in-a-slot
description: "Use before running a project's checks, its linter or its tests from a wall: factory test and its forms, the slot it waits for, what is refused, where to log a long run, and what verification runs instead."
---

# Testing in a slot

Tests and checks run in a slot the engine hands out, so that cards don't starve each other's machine.
A researcher's or baseline seat's run takes a project slot and a machine test run, but not its card's slot.

- **The project's tests of what you touched:** `factory test --run <program> [arguments…]`, the exact command you want as separate words, never one quoted string, for a narrow crate, test or filter (`factory test --run cargo test -p factory-core --test fac95_gate`).
  Run it from the clone's root: it waits for a slot, then runs in your wall where you started it.
  Queued, it prints `factory test: waiting for a slot (…)` and says nothing more until it runs: that is a queue, not a hang, and it keeps waiting across a reload of the factory, `--run`, `--file` and `--checks` alike.
- **One file's tests:** `factory test --file <path>…`, where the project has a one-file command.
- **One wall-test file of a full factory card:** its foreman may use `factory test --walls [--no-default-features] <file> [<filter>]`.
  Every other seat is refused.
  Name the file alone (`t1_walls`), with no path, extension, wildcard or flag, and at most one nonempty filter word that is no flag.
  The engine checks these words before a slot, then runs `cargo test -p factory-app --test <file>` in the card's clone outside every wall, with walls enabled and `faults`, or the production build with `--no-default-features` before the file.
  It prints the output at the job's end and exits with its code; a failure raises no card ask and no pass is remembered.
  A connection closing, the seat's turn ending, a reload or a restart stops it without replay, freeing the slot once the job is gone: request it again.
  Verification still runs both phases.
- **A JavaScript proof kept under `.git/`** runs with plain `node`, written with `node:test` or `assert`, never as a Jest test, which never looks inside `.git/`: `factory test --run node .git/factory/review-proofs/round-<n>/<proof>.js`, from the clone's root.
- **The checks gate, before you hand on:** `factory test --checks`, the engine's checks stage on your clone as it stands, uncommitted changes and new files included, so you can run it before you commit.
  Clean here is a clean checks stage once that work is committed.
- **The project's linter:** use `factory test --checks` where checks are configured; the factory project's are rustfmt, the comment rules, clippy, the audits and `web/` (typecheck and build).
  A bare linter command (`npx eslint`) may pick up a global one with no config, and there is no `--lint` form.
- **Not the suite:** only verification runs a project's whole suite, in a clone of its own, once the card's gates pass, and the merge queue again when the base moved.
  Never run the suite's command through `--run` either.
- **A long run's log** goes under your working folder where git doesn't see it (`target/`, or `.git/`), never `/tmp`, which is emptied after every turn.
- **Read the project's AGENTS.md** for how its tests are run from a wall: the features a build needs, the settings a test wants, the tests that skip themselves inside a wall and why.
- **A failing test you didn't touch:** check whether it fails on the base too before changing anything; say what you found in your hand.
- **Before adding a test,** read the repository's "Tests" rules: add one only where they call for it.
