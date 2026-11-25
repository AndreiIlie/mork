<p align="left">
  <img src="docs/logo.svg" alt="Mork logo" width="44" height="44" />
</p>

# Mork

**Outgrow the single-agent terminal. Turn your Claude, Codex, and Gemini subscriptions into a parallel, sandboxed software factory.**

Website: [https://mork.sh](https://mork.sh)

Mork drives the coding CLIs you already pay for (Claude Code, OpenAI Codex, Gemini) through a deterministic Rust state machine. Every task runs in its own isolated git clone inside a Linux Bubblewrap + Landlock sandbox, gets audited by fresh read-only reviewers from a different model family with zero builder chat history, and lands through a serialized merge queue after full checks and tests pass.

![Mork Board](docs/screenshots/board.png)

## Why Mork exists

Over 39 days of daily use (1,049 shipped work cards across 4 production and personal repositories), **45.8% of AI-written branches failed independent review after local tests were 100% green**, averaging 3.24 findings per failed branch (98.7% verified fixed before merge).

A single agent reviewing its own diff stays anchored on why its implementation is "right." Mork separates those roles structurally in code, not politely in a prompt:

- **Isolated sandboxes ("walls"):** Each card runs in its own git clone inside Linux Bubblewrap + Landlock (`crates/engine/src/wall/`). Reviewers inspect a clean read-only clone with no builder history.
- **Cross-family pairing:** Different model families plan, build, challenge plans, and review code, configured in `factory.toml`.
- **Proof-required findings:** Reviewers cannot wave at hypothetical bugs. Every finding carries an exact `file:line`, failure path, and proof; failed rounds route to structured triage (`Fix`, `Decline`, `Foreman decides`).
- **Raised Hands:** Agents only interrupt you when a real product decision, visual mock approval, plan approval, or failed-round triage is needed.
- **Serialized merge train:** Approved branches verify checks, review rounds, and full test suites side by side, then land serially onto base so parallel cards never trample each other.
- **Live web UI:** Local React + TypeScript dashboard served over a keyed localhost bridge (`web/`), featuring a full-width Board, 50/50 Card Room with auto-opened Seat Chat, live test-suite progress, and Usage / Quota pace tracking.

![Mork Card Room](docs/screenshots/card-room.png)
![Mork Usage and Quotas](docs/screenshots/usage.png)

## Architecture

Mork is a pure-Rust workspace with a strict dependency graph audited by `cargo xtask checks` (`crates/xtask/src/graph.rs`):

| Path | Role |
| --- | --- |
| `crates/types/` | Wire protocol, view models, config schema, and domain identifiers (`CardId`, `AskId`, `JobId`). |
| `crates/core/` | Pure deterministic state machine: lifecycle, review rounds, triage, merge queue, door SQL/file gates, and view builders (zero I/O). |
| `crates/engine/` | Runtime loop, SQLite event journal (`store/`), Bubblewrap + Landlock sandboxing (`wall/`), CLI drivers (`cli/`), live test-suite watcher, Vite visual mock host, and web bridge (`web/`). |
| `crates/app/` | Main `factory` binary, CLI control client (`factory ctl`), and embedded roles/manuals/skills. |
| `crates/xtask/` | Workspace verification: rustfmt, clippy, crate-graph checks, file/function size caps, web Tailwind/TS audits, and parallel test runner. |
| `web/` | Local web UI (React 19, TypeScript, Tailwind CSS v4, Vite). |
| `manuals/`, `roles/`, `skills/` | Operating manuals and on-demand skills loaded into agent seats. |

> **Naming note:** Mork started as an internal personal tool codenamed `factory`. Crate names, the binary (`factory`), and `~/.config/factory/factory.toml` still use that name while the project transitions publicly to Mork.

## Quick Start (Linux)

### Prerequisites

- **Linux** with user namespaces enabled and `bwrap` (Bubblewrap) installed
- **Rust** toolchain (`cargo`, `rustc`)
- **Node.js 22+** (for building `web/`)
- One or more supported agent CLIs installed and signed in (`claude`, `codex`, etc.)

### Build and Run

```bash
# 1. Clone the repository
git clone https://github.com/AndreiIlie/mork.git
cd mork

# 2. Build the web UI
cargo xtask web

# 3. Copy and edit the example config
mkdir -p ~/.config/factory
cp factory.example.toml ~/.config/factory/factory.toml
# Edit ~/.config/factory/factory.toml with your repo, board, and toolchain paths

# 4. Build and run the engine
cargo build --release -p factory-app
./target/release/factory
```

Open the keyed localhost URL printed by `./target/release/factory web` to access the web UI, or control cards directly from the CLI:

```bash
factory ctl card.create my-app "Short title" "Problem statement and evidence"
factory ctl start <card-id>
factory status
```

## Development Checks

```bash
cargo xtask web        # Tailwind class audit, TypeScript typecheck, and Vite production build
cargo xtask check      # rustfmt, comments, clippy, crate graph, size caps, and web checks
cargo xtask test       # Parallel workspace test runner
```

## License

[MIT](LICENSE)
