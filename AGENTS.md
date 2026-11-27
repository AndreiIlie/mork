# AGENTS.md

Instructions for anyone, human or agent, working in this repository. Instruction files here are `AGENTS.md`, never `CLAUDE.md`.

## What this is

Mork: one Rust app that runs the operator's cards.

## Who changes what

A card changes only what its agreed specification asks; this says, of each path, whether that change is the card's to make or a question to the operator first, even to loosen a limit.

| Path | Whose | Why |
|---|---|---|
| `crates/` | a card's | the code |
| `web/` | a card's | the web UI, designed to web/DESIGN.md |
| `docs/journal.md` | a card's, changed with the journal's code | read on demand through the pointer |
| `Cargo.toml`, `Cargo.lock`, `.cargo/`, `.gitignore` | a card's | the build |
| `manuals/reference.md` | a card's, changed with the commands it describes | compiled in: `mork manual reference` and the commands' help print it |
| `manuals/mork.md`, `roles/`, `skills/` | a card's, to keep their text true to what its agreed change does; a new rule or procedure only when the card agreed it with the operator | compiled in, every seat reads them |
| `orchestrator-skills/` | a card's, to keep their text true to what its agreed change does; a new rule or procedure only when the card agreed it with the operator | linked into orchestrator CLIs, never compiled in |
| `AGENTS.md`: "Build, test, run" | a card's, kept true to what it changes | how the code is built and tested |
| `AGENTS.md`: this table, "Tests", "Rules the code is held to (1.12)", "Comments"; `clippy.toml`, `rustfmt.toml` | the operator's | the rules the code is held to |

## Build, test, run

```
cargo build --workspace
cargo xtask web-types      # regenerate web/src/types.ts from the Rust wire types
cargo xtask web            # install web/ when needed, test and run the class checker, typecheck, and build web/dist
cargo xtask check          # generated web types, rustfmt, the comment rules, clippy -D warnings (all features, then the suite's production test targets without faults), the audits (1.12.8), and web/
cargo xtask checks --base <b> --json <report> # the same checks, with located findings and advisory changes to existing Rust tests and assertions against base
cargo xtask test           # verification's and the merge queue's alone: the suite once with `faults`, its test binaries side by side, each pass's test count printed first and each test's result as it lands, timed (T6.8), the build without it and its production-only tests, then the hygiene step; with MORK_TRAIN_PHASE=walled its skipped wall tests are recorded, and =unwalled runs them alone, timed within 3 minutes
cargo run -p mork-app -- --no-window
MORK_WEB_PORT=<port> npm --prefix web run dev # web UI dev server by hand, proxying /ws to the bridge
```

The app's wall and door tests need `/usr/bin/bwrap` and a Landlock ABI of 6 or more; they run walled. Each makes its own `t-*` home in a short workspace-specific folder (`.t` and six hex digits) under the test's HOME, the seat's home in a slot or the shared home during verification, outside the wall's private temporary folders, and removes it. `mork-core`'s tests run `decide` in memory, with no wall and no home.

Inside a card's wall (a foreman of a Mork card), test what you touched in a slot, with `mork test --run`:

- `mork test --run cargo test -p <crate> --test <file> [<filter>]`. When this repository is configured as `[mork] project`, the engine runs it with `MORK_UNWALLED=1` (no wall can start inside a wall) and, for a `cargo test` that builds the app and names no features, the app's `faults`: without it the app's tests compile to nothing and pass running none. When `[mork] project` is unset (or names another project), `--run` passes the command unchanged without those settings or `faults`. Name `--no-default-features` to run the production build's tests instead (`--test <file> production`). Through `sh -c` the command gets the settings (when configured as `[mork] project`) but not the feature: name it yourself.

The wall-asserting tests skip themselves there, saying why, and a test waiting on a turn whose wall couldn't start fails at once with bwrap's words. The suite is verification's and the merge queue's: `mork test`, with arguments or none, is refused. Don't run `cargo xtask test` yourself, through `mork test --run` or not: with MORK_TRAIN_PHASE unset it runs the whole suite. When this repository is configured as `[mork] project`, verification runs the suite walled first, each skipped wall test recorded under `target/wall-tests/`, then those tests alone outside every wall on the same candidate, and the card lands only once both passed; a record the second phase can't read, or a recorded test its rebuilt binary doesn't list, fails it. When `[mork] project` is unset, verification runs the suite once walled with no second unwalled phase. A wall test that skips in the second phase currently passes. `mork test --checks`, committed or not, checks that `web/src/types.ts` matches the Rust wire types and runs rustfmt, comments, clippy, the audits and `web/` (`cargo xtask checks`) from the clone's own code and reports located findings: run it before handing on. Log a long run under `target/`, never `/tmp`, which is emptied after every turn.

A full card's foreman on the configured `[mork] project` may run one wall-test file with `mork test --walls [--no-default-features] <file> [<filter>]`. When `[mork] project` is unset, or for any other seat, `--walls` is refused. The file is a plain test-file name (`t1_walls`), without a path, extension, wildcard or flag; the optional filter is one nonempty word that is no flag, checked in core before a slot. The engine runs `cargo test -p mork-app --test <file>` from the clone as it stands outside every wall, with `faults` and `MORK_UNWALLED` unset. `--no-default-features` before the file selects the production build. The command waits for its output and exits with its code; a failure raises no card ask and no pass is kept. Closing the connection, ending the turn, a reload or a restart stops it without replay, and its slot is freed once the job is gone. When `[mork] project` is configured, verification still runs both phases.

## Tests

Four things are tested: the wall (what a seat's job can see, reach and write), the door (it runs nothing without its approval, passes arguments as literal words and leaks no credential), pure state-machine transitions in `mork-core` (`decide` in memory, with no filesystem, git, or sandbox), and narrow unit or isolated temp-dir tests in `crates/engine` for parsers, symlink/filesystem safety helpers, and in-flight turn recovery. Recovery fixtures replay saved job folders and state in a temporary directory without starting processes or waiting on timers.

No other integration test is added: none for crash recovery, timing, or the window. A change to the wall, the door, a `mork-core` transition, or a tested `crates/engine` parser, safety helper, or turn recovery path keeps its tests true. When a test flakes, fix the source of nondeterminism or use a deterministic fixture that exercises the same behavior. Never mask a flake with sleeps or retries.

## Rules the code is held to (1.12)

- `core` is pure: no clock, randomness, file, process, network or database.
- One module per outside world in `engine`; processes start only in `engine::jobs`; `unsafe` only in `sys`.
- No non-test Rust file over 500 lines and no non-test Rust function over 60 lines or over cognitive complexity 15 (Rust test files and `web/` TypeScript are exempt from size caps); no `unwrap`/`expect`/`panic!` outside tests, no unfinished-work markers but a TODO or FIXME carrying a card id, every `#[allow]` with a `// why:` line above it.
- The engine never runs git with an agent's clone as its repository.
- The engine makes, writes and moves paths inside an agent's clone, `.git/` included, only through `sys`'s `mkdir_beneath` (one level per call), `write_beneath`, `append_beneath` and `move_dir_beneath`, rooted at a path of its own (the clone folder, a home folder), as `runtime/handoff.rs`'s `put_back` does: a seat can plant a link anywhere in its clone, and `std::fs` follows it. Never `open_no_symlinks` or `open_beneath_no_symlinks` there: they refuse a link anywhere in the absolute path, so they fail on a home reached through a link, and are for configured paths.
- No backward-compatibility shims for older builds' live state: Mork is a single-user tool, so breaking live-state changes drain active cards before reloading (or run a one-shot migration removed afterward) instead of keeping retired variants and recovery paths in Rust code. Only completed historical cards' journal rendering stays readable.
- Each public release documents which versions users can upgrade from and when they must start fresh.

## Comments

Explain the reason or invariant in each new comment. A reader should understand it without opening a card. A run of consecutive standalone line comments may exceed 2 lines when its first line begins with `// SAFETY:` or `// INVARIANT:`. Keep all other comments to 2 lines and put longer explanations in the card note. If you need to explain what, change the code. Doc comments only on pub items, stating the contract. TODOs carry a card id. The checks enforce the length, placement and card ID rules.
