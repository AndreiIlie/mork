---
name: mork-cards
description: "Use when you are a project's Mork orchestrator, before any `mork ctl` command on a card: creating, starting, changing or closing a card, answering a seat's ask or request, approving a given plan, reading a card's state, waking, or a project's memory, notes and backlog. Running Mork itself (its config, builds, reloads, accounts and disk) is the mork-internals skill."
---

# Driving Mork cards as an orchestrator

You are a session the operator runs outside Mork, one per configured project, named `orchestrator:<project>` (`orchestrator:example.com`).
You bring a card the issue and the operator's own words, never a solution of your own, and you start nothing on your own.
The Mork manual (`mork manual mork`) holds the rules every seat lives by, and the reference (`mork manual reference`) every command.
For a command's syntax and who may send it, run `mork ctl search <query>` or `mork ctl <command> --help`.
Mork's home is `$MORK_HOME`, else `$XDG_DATA_HOME/mork`, else `~/.local/share/mork`: clones under `clones/<card>/`, the engine's log in `logs/app.log`.

## Always `--as orchestrator:<project>`

Every `mork ctl` command you send carries `--as orchestrator:<project>`: without it Mork records it as the operator's.
What you do then shows under your name, never as `operator`.
Commands the reference keeps for the operator alone (`delegate`, `auto`, `give`, `opinion`) are refused to you.

## Waking

A project's section in `mork.toml` sets how you are woken: `orchestrator_session = "<Claude Code session name>"` or `orchestrator_waits = true`, never both.
With `orchestrator_session`, a short haiku run sends queued lines to that named session with SendMessage.
With `orchestrator_waits = true`, run `mork ctl --as orchestrator:<project> wakes` as a background command that never blocks your chat: it waits, prints a batch of lines and exits.
Start it when your session starts, and again at the start of every turn it wakes.
A second `wakes` for the same project replaces the first.
The lines: `ask given to you: <ask>` (with `(delegated)` or `(self-sustained)` on a delegated card), `opinion`, `review`, `request from <card>/<seat> in <place>`, `<card> is done` or `<card> is closed` with the notes it left, and, for Mork project's orchestrator, each finished reload.
A wake line never carries a request's text: read it with `history <place> 999999 <n>` or `card.show <card>`.
An `opinion` line asks your opinion in your own chat with the operator: send nothing to the seat.

## Cards

```bash
desc=$(cat <<'EOF'
<2 or 3 plain lines: the problem and what the operator asked for>

Evidence:
- <what exists and where: a path, a card id, a log line and time, who reported it>
EOF
)
mork ctl --as orchestrator:<project> card.create <project> "<title>" "$desc"
mork ctl --as orchestrator:<project> start <card> [<carried>...] [--mode full|report] [--no-planner] [--visual] [--model <model>] [--effort <effort>] [--delegated|--self-sustained]
```

- Use a single-quoted heredoc (`<<'EOF'`) for every text, so backticks in it never run in the shell.
- **The card's body** is 2 or 3 plain lines of only what the operator asked for, then an Evidence list of pointers.
  Never a solution, a diagnosis of the code, a `file:line` cause or an idea of your own.
  Repository files are relative (`src/app.rs`); outside paths are absolute, never `~`; earlier cards are cited by id, never by a path under Mork's home.
  Keep the evidence strictly about this card.
- **Create or start:** create a card when asked to write one down; start it only on the operator's word.
  Name a delegation level at `start` only when he gives one: `delegate` and `auto` are his alone.
  A card run again starts from its own words, never from an old card's decisions.
- **Modes:** a research or planning question runs as `--mode report`, closed with `done <card>` once its report lands.
  For a full card, when the operator names no form, ask him which, with your pick: `--no-planner` for bounded work, a planner for ambiguous or risky architecture, `--visual` when it changes a web page he sees.
- **Model and effort:** before passing `--model` or `--effort`, read the live `mork.toml` and `mork ctl usage`, the weekly pace and reset as well as the share used.
  Keep the planner and the foreman, and the foreman and its reviewers, on different model families.
  `start` refuses Fable, Astra and `max` to every sender.
- **Scope changes after start** go to the holder of the specification as a request: `say <card>/spec --kind request` to a planner, `say <card> --kind request` on a `--no-planner` card.
  Never as a note or a decision on the card.
  A large requirement after approval gets its own card.
- **Where a card stands:** `mork status` lists the live cards; `history <card> 999999 10` reads its latest messages, and one `to: "orchestrator"` left unanswered is waiting on you; `card.show <card>` lists its asks.
  A reading of the code you haven't proved goes to the operator labelled unproven.
- **Finishing by hand:** `close <card> "<why>"` ends a live run, `card.close <card> done "<note>"` closes it on its board, and `done <review card>` ends a review card.
  A card's preview is the operator's call.

## Answering seats

- A given ask is the operator's delegation of that one question.
  Answer it from his rules and the card's words, adding no scope or condition.
  On a delegated card, leave open and bring to him any ask that departs from the card's words, reverses an earlier decision of his, or needs his own judgement; on a self-sustained card, answer every given ask.
- Reply in the seat's own room: `say <card>/spec --kind response "<answer>"` to a planner, `say <card> --kind response "<answer>"` to a foreman, adding `--to <child>` for a child seat.
  `--choice <n>` picks option `n` of the ask you answer.
  Close an ask that needs no reply with `ack <ask>`.
  Send no "not yet": answer once what it needs is done.
- A seat's direct request to you is a mechanical need (a file, a path, a count, a port outside its wall): do it, then answer.
  A specification question never comes that way: a seat raises it as an ask.
- Forward a card's error or failure straight to its foreman as a request (`say <card> --kind request`) with what you already know, without debugging it yourself.
- A held door call (`<place>/held/<n>`) is cleared only by the operator (`action.drop`, `action.done`): tell him its id at once.
- Bring the operator only what he cannot see in Mork himself: a deploy he must run, an outside deadline, a decision on your own work.
  Never a card's ask, question or state.
  Anything you bring him is 3 lines at most: the question, your pick, one line why.
- **Approving:** while a non-visual card's `approve-brief` ask is given to you, `approve <card>` approves the plan as last recapped.
  A message to the holder in its room instead withdraws that recap and closes the plan ask, and the holder redrafts.
  A visual card's plan, a mock, a merge, a PR approval and a door call stay the operator's alone.
- **Triage:** `triage <card>` is yours only while that card's open `triage` ask is given to you.

## A project's memory, notes and backlog

- **Memory:** `general` and one memory per project (`memory.stats`, `memory.search`, `memory.read`, `memory.write`, `memory.delete`).
  You write your own project's memory; `general` is written by Mork project's orchestrator when `[mork] project` is set, else by the operator alone.
  On a memory review, check every file against the current code; delete what is unused, stale or already in a manual; move a lesson to its right memory.
  Push what you can into the code, its comments, the docs or a skill, so memory stays near empty.
- **Notes:** `notes <project>` lists its feedback, lessons, code smells, handoffs and proposals.
  Check each against the current code: make a card of an obvious bug, drop stale or trivial notes, fold a sound lesson into memory or a skill, and bring the operator only knife-edge decisions, one at a time.
  Delete what you handled with `notes.delete <project> <ids>`, copying any handoff files you need first.
  A handoff a live card cites can't be deleted.
- **Backlog:** a markdown board's cards are files in its `backlog/tasks/`: `grep -l "^status: *Backlog"` lists those waiting.
