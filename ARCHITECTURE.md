# Mork Architecture

Mork is a local Linux orchestrator (~89k LOC Rust across 8 crates, ~5.5k LOC React/TypeScript) built around one design rule: **separate pure orchestration logic from impure OS/CLI boundaries, and enforce that separation in CI (`cargo xtask check`).**

## 1. Crate Dependency Graph

`crates/xtask/src/graph.rs` enforces the workspace dependency DAG and capability boundaries at check time:

```text
                ┌───────────────┐
                │   factory-app │  binary, CLI client (ctl/test), embedded manuals/roles/skills
                └───────┬───────┘
                        │
          ┌─────────────┴─────────────┐
          ▼                           ▼
   ┌────────────┐              ┌─────────────┐
   │ factory-ui │              │factory-engine│  runtime loop, SQLite journal, bwrap wall,
   │ (desktop)  │              │             │  CLI drivers, door, localhost web bridge
   └─────┬──────┘              └──────┬──────┘
         │                      ┌─────┴──────┐
         ▼                      ▼            ▼
  ┌──────────────┐       ┌────────────┐  ┌─────────────┐
  │ factory-core │       │ factory-sys│  │factory-fakes│ (tests only)
  │ (pure logic) │       │ (unsafe)   │  └─────────────┘
  └──────┬───────┘       └────────────┘
         │
         ▼
  ┌───────────────┐
  │ factory-types │  wire protocol, config schema, view models, IDs
  └───────────────┘
```

Enforced workspace invariants (`Cargo.toml` + `crates/xtask/`):
- `crates/core` has zero clock, filesystem, network, process, or SQLite calls.
- All `unsafe` blocks are confined to `crates/sys` (Landlock syscalls and `openat`-based symlink-safe file ops).
- Process spawning is confined to `crates/engine/src/jobs/`.
- Production Rust denies `unwrap_used`, `expect_used`, `panic`, `todo`, and `undocumented_unsafe_blocks`.
- Non-test Rust files are capped at 500 lines and functions at 60 lines (cognitive complexity <= 15).

## 2. Pure State Machine (`core::decide`)

Every lifecycle transition (card start, spec brief, plan challenge, approval, build submission, review round dispatch, finding triage, stalemate detection, merge-queue verification, and landing) runs through a pure reducer in `crates/core/src/decide.rs`:

```text
(State, Event) -> (NewState, Vec<Effect>)
```

Because `core` has no I/O, the full state-machine suite in `crates/core/tests/` executes in memory in milliseconds without git, SQLite, or subprocesses. `crates/engine/src/runtime/` persists events to the SQLite journal (`crates/engine/src/store/`), executes the returned effects, and feeds completion events back into `decide`.

## 3. Card Lifecycle & Cross-Vendor Separation

```text
Card (Plan / Visual flags)
  ├──► Planner (spec room, read-only clone + optional .git/factory-mock/)
  │      ▲   ▲
  │      │   └── Plan recap challenge ──┐
  │      ▼                              ▼
  │     You ◄── Raised Hands ───► Foreman (build room, writable isolated clone)
  │                                 ├── Baseline (visual cards: today's UI snapshot)
  │                                 └── Researcher (read-only bounded scout)
  │                                         │
  │                                  checks & narrow tests ok
  │                                         ▼
  │                               Review Foreman (separate review card)
  │                                 └── 4 parallel blind reviewers (different vendor,
  │                                     read-only clone, zero builder chat history)
  │                                         │
  │                     findings ───────────┴────────── pass
  │                        │                             │
  │                        ▼                             ▼
  │              Fix loop back to Foreman        Serialized Merge Queue
  │                                            (two-phase walled + unwalled suite)
  │                                                      │
  │                                                      ▼
  └──────────────────────────────────────────────────► main
```

Key properties:
- **Card as memory:** Agreed specification decisions are recorded directly on the card (`card.change --decision`) so fresh sessions, compacted sessions, and blind reviewers read the exact same source of truth.
- **Raised Hands:** Agents stay silent during normal execution and surface a structured ask (`answer`, `approve-brief`, `triage`, `blocked`) only when human judgment is needed.

## 4. Detached File-Backed Turns & Plug-and-Play CLI Adapters

Every agent turn runs under a detached supervisor process (`crates/engine/src/jobs/supervisor.rs`) backed by a plain job directory (`crates/engine/src/jobs/folder.rs`):

```text
jobs/<job-id>/
  ├── job.json          # immutable launch spec
  ├── stdin             # opening prompt
  ├── input/<seq>.json  # mid-turn messages delivered while the turn runs
  ├── out.jsonl         # raw CLI stream output
  ├── out.stamps        # read watermarks
  └── exit.json         # final exit code and status
```

A non-blocking feed (`crates/engine/src/jobs/feed.rs`) watches `input/` and translates messages through a thin per-CLI driver (`crates/engine/src/cli/live.rs`, `claude.rs`, `codex.rs`, `agy.rs`):
- **Claude Code:** stages mid-turn messages on live `stream-json` stdin with `shouldQuery: false`.
- **Codex:** steers active turns over the Codex app-server JSON-RPC stream.
- **Gemini (`agy`):** queues messages for the next turn while parsing the structured event stream.

This single file boundary provides two guarantees:
1. Adding a new CLI vendor requires one small adapter and stream parser in `crates/engine/src/cli/`.
2. Reloading or restarting the engine does not kill running agent turns; the detached supervisor continues writing `out.jsonl`, and the restarted engine replays `out.jsonl` idempotently (`StreamState`).

## 5. Sandbox Wall, Symlink Defense & Unix-Socket Door

- **Wall (`crates/engine/src/jobs/wall.rs`, `wall_exec.rs`, `crates/sys/src/landlock.rs`):** Every turn launches inside Bubblewrap (`--unshare-user/pid/ipc`, `--cap-drop ALL`, `--clearenv`, read-only host root, tmpfs over host `$HOME`) and installs a Landlock TCP connect filter before exec. If Landlock ABI support is missing, production builds fail closed (`refusing to run unwalled`).
- **Symlink-safe clone I/O (`crates/sys/`):** Because a seat controls its git clone, the engine never runs `git` with an agent clone as its own repo and never uses raw `std::fs` paths inside a clone. Writes go through `openat`-rooted `mkdir_beneath`, `write_beneath`, `append_beneath`, and `move_dir_beneath` so planted symlinks cannot escape the clone directory.
- **Door (`crates/engine/src/door/`):** External database, log, and SSH operations run outside the wall via a per-turn authenticated Unix socket (`factory ctl door`). Read ops check `targets.toml` allowlists; `misc.run` holds for operator approval; credentials stay outside the wall and outputs pass through `door/redact.rs`.
- **Why a Unix-socket CLI instead of MCP inside the wall?** A CLI with on-demand `--target <t> --help` avoids injecting large tool schemas into every turn's prompt, keeps prompt-cache hit rates high, authenticates turn identity through the socket, and works identically across every vendor CLI.

## 6. Testing & Two-Phase Wall Verification

Mork tests four high-leverage seams (`AGENTS.md`):
1. Pure `core::decide` transitions in memory (`crates/core/tests/`).
2. The sandbox wall (`crates/app/tests/t1_walls.rs` and `m0_walled_turn.rs`), including production-build checks that test overrides compile out.
3. The door (approval gating, literal argv passing, credential redaction).
4. Narrow `crates/engine` parsers and symlink-safety helpers.

During implementation, build seats run only narrow touched tests (`factory test --file` / `--run`) and `factory test --checks`. When landing Mork itself, verification runs the full suite in two phases (`FACTORY_TRAIN_PHASE=walled` recording skipped wall tests under `target/wall-tests/`, followed by `=unwalled` executing those exact recorded tests outside every wall on the same candidate commit).
