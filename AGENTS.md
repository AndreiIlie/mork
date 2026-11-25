# AGENTS.md

Instructions for anyone, human or agent, working in this repository. Instruction files here are `AGENTS.md`, never `CLAUDE.md`.

## What this is

Factory v2: one Rust app that runs Andrei's cards.

## Who changes what (fac-167)

A card changes only what its agreed specification asks; this says, of each path, whether that change is the card's to make or a question to Andrei first, even to loosen a limit.

| Path | Whose | Why |
|---|---|---|
| `crates/` | a card's | the code |
| `web/` | a card's | the web UI, designed to web/DESIGN.md |
| `docs/journal.md` | a card's, changed with the journal's code | read on demand through the pointer |
| `Cargo.toml`, `Cargo.lock`, `.cargo/`, `.gitignore` | a card's | the build |
| `manuals/reference.md` | a card's, changed with the commands it describes | compiled in: `factory manual reference` and the commands' help print it |
| `manuals/factory.md`, `roles/`, `skills/` | a card's, to keep their text true to what its agreed change does; a new rule or procedure only when the card agreed it with Andrei | compiled in, every seat reads them |
| `AGENTS.md`: "Build, test, run" | a card's, kept true to what it changes | how the code is built and tested |
| `AGENTS.md`: this table, "Tests", "Rules the code is held to (1.12)", "Comments"; `clippy.toml`, `rustfmt.toml` | Andrei's | the rules the code is held to |

## Build, test, run

```
cargo build --workspace
cargo xtask web            # install web/ when needed, typecheck, and build web/dist
cargo xtask check          # rustfmt, the comment rules, clippy -D warnings, the audits (1.12.8), and web/
cargo xtask checks --base <b> --json <report> # the same checks, with located findings for the checks gate
cargo xtask test           # verification's and the merge queue's alone (fac-65, fac-375): the suite once with `faults`, its test binaries side by side, each pass's test count printed first and each test's result as it lands (fac-697), timed (T6.8), the build without it and its production-only tests, then the hygiene step; with FACTORY_TRAIN_PHASE=walled its skipped wall tests are recorded, and =unwalled runs them alone, timed within 3 minutes
cargo run -p factory-app -- --no-window
FACTORY_WEB_PORT=<port> npm --prefix web run dev # web UI dev server by hand, proxying /ws to the bridge
```

The app's wall and door tests need `/usr/bin/bwrap` and a Landlock ABI of 6 or more; they run walled. Each makes its own `t-*` home under a short workspace-specific folder in the shared agent home (`.t` and six hex digits), outside the wall's private temporary folders, and removes it. `factory-core`'s tests run `decide` in memory, with no wall and no home.

Inside a card's wall (a foreman of a factory card), test what you touched in a slot, with `factory test --run`:

- `factory test --run cargo test -p <crate> --test <file> [<filter>]`. On this project the engine runs it with `FACTORY_UNWALLED=1` (no wall can start inside a wall), `FACTORY_TEST_PORTS` set to the project's first port range (where the fakes listen), and, for a `cargo test` that builds the app and names no features, the app's `faults`: without it the app's tests compile to nothing and pass running none. Name `--no-default-features` to run the production build's tests instead (`--test <file> production`). Through `sh -c` the command gets the settings but not the feature: name it yourself.

The wall-asserting tests skip themselves there, saying why, and a test waiting on a turn whose wall couldn't start fails at once with bwrap's words. The suite is verification's and the merge queue's (fac-10, fac-65, fac-375): `factory test`, with arguments or none, is refused. Don't run `cargo xtask test` yourself, through `factory test --run` or not: with no FACTORY_TRAIN_PHASE it still runs the whole suite until fac-123 refuses it. Verification runs it walled first, each skipped wall test recorded under `target/wall-tests/`, then those tests alone outside every wall on the same candidate, and the card lands only once both passed; a record the second phase can't read, or a recorded test its rebuilt binary doesn't list, fails it. Until fac-123 a wall test that skips in the second phase still passes. `factory test --checks`, committed or not, runs rustfmt, comments, clippy, the audits and `web/` (`cargo xtask checks`) from the clone's own code and reports located findings: run it before handing on. Log a long run under `target/`, never `/tmp`, which is emptied after every turn.

A full factory card's foreman may run one wall-test file with `factory test --walls [--no-default-features] <file> [<filter>]` (fac-480). All other seats are refused. The file is a plain test-file name (`t1_walls`), without a path, extension, wildcard or flag; the optional filter is one nonempty word that is no flag, checked in core before a slot. The engine runs `cargo test -p factory-app --test <file>` from the clone as it stands outside every wall, with `faults`, `FACTORY_UNWALLED` unset and the project's test ports. `--no-default-features` before the file selects the production build. The command waits for its output and exits with its code; a failure raises no card ask and no pass is kept. Closing the connection, ending the turn, a reload or a restart stops it without replay, and its slot is freed once the job is gone. Verification still runs both phases.

## Tests

Four things are tested: the wall (what a seat's job can see, reach and write), the door (it runs nothing without its approval, passes arguments as literal words and leaks no credential), pure state-machine transitions in `factory-core` (`decide` in memory, with no filesystem, git, or sandbox), and narrow unit or isolated temp-dir tests in `crates/engine` for parsers and symlink/filesystem safety helpers.

No other integration test is added: none for crash recovery, timing, or the window. A change to the wall, the door, a `factory-core` transition, or a tested `crates/engine` parser or safety helper keeps its tests true. A test that flakes is deleted.

## Rules the code is held to (1.12)

- `core` is pure: no clock, randomness, file, process, network or database.
- One module per outside world in `engine`; processes start only in `engine::jobs`; `unsafe` only in `sys`.
- No non-test Rust file over 500 lines and no non-test Rust function over 60 lines or over cognitive complexity 15 (Rust test files and `web/` TypeScript are exempt from size caps); no `unwrap`/`expect`/`panic!` outside tests, no unfinished-work markers but a TODO or FIXME carrying a card id, every `#[allow]` with a `// why:` line above it.
- The engine never runs git with an agent's clone as its repository.
- The engine makes, writes and moves paths inside an agent's clone, `.git/` included, only through `sys`'s `mkdir_beneath` (one level per call), `write_beneath`, `append_beneath` and `move_dir_beneath`, rooted at a path of its own (the clone folder, a home folder), as `runtime/handoff.rs`'s `put_back` does: a seat can plant a link anywhere in its clone, and `std::fs` follows it. Never `open_no_symlinks` or `open_beneath_no_symlinks` there: they refuse a link anywhere in the absolute path, so they fail on a home reached through a link, and are for configured paths.
- No backward-compatibility shims for older builds' live state: Factory v2 is a single-user tool, so breaking live-state changes drain active cards before reloading (or run a one-shot migration removed afterward) instead of keeping retired variants and recovery paths in Rust code. Only completed historical cards' journal rendering stays readable.

## Comments

Comments say why, never what: a reason, an invariant, a warning, or a card pointer. At most 2 lines; longer belongs in the card note. If you need to explain what, change the code. Doc comments only on pub items, stating the contract. TODOs carry a card id. The checks enforce this.
