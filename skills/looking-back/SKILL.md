---
name: looking-back
description: "Use for Pulse and Deep Sweep, the project look-back reads that leave grouped cleanup proposals."
---

# Looking back

Read `mork ctl card.show <card>` for the read, fixed commit and input file.
Keep using that commit if `origin/<base>` moves.
The file is under `.git/mork/`, read-only in the report wall.
Change no code and write no card.

## Method

- Split merged changes into pieces of about three large changes or fifteen small ones.
  For a whole-code read, use module boundaries.
  Give each researcher the fixed commit and the files, changes or cards it owns.
- In each task say what counts as a find and what falls outside the read.
  Require the whole of every assigned file or hunk, including tests and deleted code, and a list of anything left unread.
  Send unread parts back before reporting.
  Check every candidate at the fixed commit and at its cited source when that source is outside the tree.
- Keep files needed next turn in `.git/mork/handoff/`, the writable folder in the report wall.
  Remove scratch files before the final report.
  Files left there become a handoff note.

## Pulse

- Enumerate every commit in `<from>..<through>` with `git log --topo-order --reverse --format='%H %P %s' <from>..<through> --`.
  Match commits to the input file's `landings` and read their recorded before and after diffs.
  Include direct commits, fast-forwards and changes without a card.
  Look for mechanisms that can now be removed or replaced by something already present.
- If the description gives a seven-day cutoff and lacks a starting commit, ask the project's orchestrator for the base movements at that cutoff through the fixed commit, from the checkout's reflog or hosting service.
  Use when changes entered the base.
  If a bound remains unavailable, report what you could read and what is missing.
- Use `card.show` and `history` for landed cards and their linked review rooms.
  Page back from the oldest message's `seq` and use `history.journal` for older runs.
  The input file's `unplaced_landings` lists records whose landed commit is unknown.
  Report relevant gaps and use the available Git history to cover the code.
- Read each `ended_cards` entry, including review cards and earlier runs of reopened cards, using its card, room and journal for the listed run.
  Find causes of wasted turns that recur on at least two cards and asks already answered by the card or a manual.
- Read the instruction files changed in the range, including AGENTS.md, manuals, roles, skills and docs.
  Find text the landed changes now contradict, duplicate or leave out of date.
- Check the `open_cards` that the landed changes touch for work already done, overtaken or duplicated.

## Deep Sweep

Split the whole code among researchers by module and cover four passes at the fixed commit.

- Read every module and its callers for **dead code**.
  Find uncalled items, one-value parameters, forwarding wrappers, copied helpers and remnants of removed mechanisms.
  Include unused dependencies and settings, accepted but ignored options, scripts and files with no callers, commented-out code, TODOs without a card or past their purpose, and comments that contradict code.
- Read every test and its support code for **dead tests**.
  Find tests that cannot fail, exercise only mocks or literals, duplicate coverage, are disabled or never discovered, or fall outside the project's testing rule.
  Include unused fixtures and private copies of shared fakes.
- Read the code and history reachable from the fixed commit for **risks**.
  Look for committed secrets, queries or commands assembled from values, missing permission checks, and private data in logs.
  Give the source line and how the code is reached.
  For a secret, give its kind and location and keep its value out of notes and replies.
- Check **wording** users see in the product and its messages.
  Find different names for the same thing, mixed languages in one flow, placeholders, leftovers and text that no longer matches behaviour.
  Name the affected places and leave the wording decision to the operator.

## Proposals and report

Group finds by the one change that would fix them.
Deduplicate against each other, the input file's `open_cards`, and the existing proposals and feedback from `mork ctl notes <project>`.
File at most seven proposals per run, strongest first, with `mork ctl notes.add proposal '<text>' --title '<title>' --file <path>`.
Add one `--file` per affected path.
Use a card-ready title and say what to change, why it matters and which finds it covers.
Cite each file-based find as `file:line` at the fixed commit, or `<commit>:<path>:<line>` for historical evidence.
For friction in ended cards, cite the cards and runs.
Do not file smell or feedback notes during a look-back.

Give a short report listing the proposal titles and any gaps, or say that the read found nothing.
Mention weaker finds beyond the seven in one line of the report.
End with `report-ready`.
The operator's `done` then records the run.
