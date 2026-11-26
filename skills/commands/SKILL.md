---
name: commands
description: "Use before sending a mork ctl command: how fields are filled, the JSON form, who a command counts as sent by, what its answer and exit codes mean, and the error codes. For one command's reference, mork ctl COMMAND --help."
---

# Commands

`mork ctl <command> --help` prints one command's signature and reference; `mork manual reference` all of them; `mork ctl --help` every signature.

- **Sending one.**
  `mork ctl <command> <fields>` sends one command to the running mork and prints its answer.
  Plain words fill the required fields in order.
  When the last required field is free text (`text`, `why`, `note`, `question`, `title`, `description`, `task`, `words`) or a list, it takes all the remaining words; otherwise the words past the required ones fill the optional fields not yet given: when the only one is free text, it takes them all, else they fill them in the catalog's order, one word each (`start ex-550 report` sets `mode`), and the last required field takes what is left.
  `start`'s words after its card are the cards its run carries (`start ex-550 ex-551 ex-552`), one card each and never split at commas, unless the first of them is a mode word: then mode, model and effort as before; with carried cards, name those three with their flags.
  `--<field> <value>` fills any field, anywhere, with the one word after it (quote a value with spaces); boolean flags (`--delegated`, `--no-planner`) take no word after them.
  `spawn`'s `--task-file <path>` option, `-` for stdin, reads its task from a file and sends the text as written, for a task with quotes or parentheses.
  Give optional fields that way to be sure.
  Only list fields split items at commas; they also take one quoted JSON array of strings.
  `deliverables`' names and `plan`'s tasks never split at commas.
  Give one item per line; `deliverables` also takes one name per argument.
  A word starting with `--` is always read as a field name: for text holding one, send the command as JSON.
- **JSON.**
  `mork ctl --json '<command>'` sends one command exactly as written, `{"<name>": {<fields>}}`.
- **Who sends it.**
  Inside a wall, `mork ctl` reaches the engine through your job's socket, and the command is yours: the engine knows which agent's job it came from.
  Outside every wall, on `mork.sock`, it is the operator's, unless an unwalled seat's job sends it, which is that seat's.
  `mork ctl --as orchestrator:<project> <command>` on `mork.sock` (`orchestrator:example.com`, `orchestrator:mork`) sends it as that project's orchestrator: a declaration, not a lock, refused from any job's socket and for a project not configured.
  A connection that belongs to no running job is refused.
- **Answers.**
  An accepted command prints its `ok` answer as one JSON object on stdout and exits 0.
  A refused one prints `{"err": {"code", "message"}}` on stdout and exits 1.
  A usage mistake, no running mork or a failed hand-over exits 2 with words on stderr.
  Exit 3 means no answer came: the command may or may not have happened, so look before sending it again.
- **Error codes.**
  `invalid`: an unknown command, an unknown or missing field, a value of the wrong type or an unknown word (a mode, a role).
  `refused`: the state doesn't allow it, in words that say why.
  `buzz`: refused because of who sent it.
  `recovering`: Mork is still starting; ask again.
  `reloading`: a reload is under way, and every command that acts outward is refused for the moment (reads go through); ask again.
  `leak`: a board write would publish a secret; `internals`: it would name Mork's internals.
  `too_large`: a line over 1 MiB.
  `replaced`: a `wakes` waiter another `wakes` replaced.
- **Builds.**
  When the running mork is another build than the `mork` binary you ran, `mork ctl` and `mork test` run the running build's binary instead, so the commands are always the running build's.
- **Authority.**
  An orchestrator stops and resumes any card, never one agent; it may send now to any agent.
  What an orchestrator does shows as `orchestrator:<project>`, never as `operator`.
  On a repository with PRs neither `merge` nor `review.merge` merges anything: only his approval on Bitbucket does.
- **Effects.**
  A command's effects are committed with its answer, in one transaction.
  A command naming a card that has ended reads it back first, and acts on it as on any other.
  A Mork announcement wakes nobody.
- **The ones you'll use most:** `card.show <card>`, `card.change <card> --decision …`, `card.change <card> --withdraw <n>`, `spawn <card> <role> '<task>' --model … --effort …`, `pick <agent> --model … --effort …`, `resolve <card> <n>` (a review card's finding verified fixed, or `--declined`, ruled out by a decision on the card — the operator's or one recorded on the orchestrator's delegated answer), `deliverables` (a foreman at "Build it"), `plan` and `plan.task` (a foreman's tasks for its deliverable), `say <place> --to <agent> --kind <kind> <text>`, `fresh <agent>`, `door.help` and `door`.
