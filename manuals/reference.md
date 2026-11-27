# The technical reference

Every command Mork takes, as the engine defines it.
Each command's signature is the engine's own catalog, shown under its heading; the text says what it does, what it answers, what refuses it and who may send it.
`mork ctl <command> --help` prints one command's section, `mork manual reference` all of them.

## How commands work

The `commands` skill says how a command's fields are filled, the JSON form, who a command counts as sent by, its answer and exit codes, and the error codes.

Boolean flags such as `status --jobs` take no value; JSON commands use `true` or `false`.
Arrays preserve spaces, commas and empty items: `mork ctl card.change ex-550 --labels '["Review, then merge","Ready"]'`.
An argument beginning with `[` is read as an array and rejected if malformed or containing non-string items.
The same field syntax works in the window's `/` commands.

## Configuration

### project

In `mork.toml`, `[mork] project` is optional (the `[mork]` table itself stays required) and names the configured project that holds Mork's own repository.
When unset, no project is Mork's own: nothing self-builds on landing or from the base tip, the suite runs once walled with no second unwalled phase, and `build`, `pin`, every `reload` and `mork test --walls` are refused.
Unset also switches off Mork project as a destination: closing turns ask `feedback` alone and refuse `mork_feedback`, cross-project `notes.add --project` is refused for every seat, finished reloads wake no orchestrator, `general` memory is written by the operator alone, and `orchestrator.review general` is refused without recording `reviewed_ms`.

### internals

In `mork.toml`, `[internals] hide` (default `false`) enables the outward provenance wording check across commit messages, branch names, pull request titles and descriptions, review comments and thread replies.
When `hide` is `true`, the engine refuses the configured `[internals] words` (with their defaults), generation markers (`Co-Authored-By:`, `Generated with`, the robot emoji), the whole word `mork` in any letter case and seat-name patterns.
When `hide` is `false`, none of those refuse anything; secret scanning and refusal of paths inside Mork's home stay on always.

### Project maintenance passes

Each pass has an optional table in `mork.toml`.
Omitted passes use `manual`.

```toml
[projects.factory.passes.pulse]
mode = "scheduled"
landings = 30
days = 7

[projects.factory.passes.dream]
mode = "auto"
budget = 60

[[picks.pass]]
cli = "codex"
model = "gpt-6.2-sol"
label = "Sol"
efforts = ["high", "xhigh"]
```

Scheduled passes require at least one positive threshold.
Auto accepts only its mode and, for Dream, its budget.
See `lookback` for how each threshold is counted.
Dream's `budget` is the maximum number of project memory files to keep and defaults to 60.
Auto requires at least one configured model and effort in the global `[[picks.pass]]` list.
Each entry names a supported CLI and model together, with efforts that model accepts and that have benchmark values.
The initial benchmark table covers Claude Fable 5.2, Opus 5.5, Sonnet 5.5, GPT-6.2 Astra and Sol, and Gemini 4 Neon.
Opus can also run on agy at low, medium or high.
A pass table containing the retired `spare` key fails to load.

### Project board and forge

Each project requires a `[projects.<name>.board]` table with `kind = "markdown"` and `path` pointing to the folder that contains `backlog/`.
An absent `[projects.<name>.forge]` table selects `local`.
A forge with `kind = "bitbucket"` requires `workspace` and `slug` and accepts an optional `review_tag`.
Bitbucket keeps the review token, its `approver=` line and the separate operator merge token.
A forge with `kind = "github"` requires `owner`, `repo` and `token`, the absolute path to a file containing only its token.
The engine uses it for Git and all API calls.
Walls mask the configured file and logs redact the credential.
Both hosted forges accept `merge = "manual"` (the default) or `merge = "approval"`.
In manual mode the operator merges on the forge and the card lands when the poller reads the merge.
To keep automatic merging on approval, existing Bitbucket projects must explicitly select `approval`.
GitHub approval merges require `approver`, the operator’s login, and a token owned by a different account.
Manual mode permits the operator's own token.
GitHub also accepts `review_tag`.
GitHub PRs from forks appear in `prs` and are excluded from review and merge.

### git_author

Set `git_author = "Alex Smith <alex@example.org>"` in `[projects.<name>]` to choose that project's Git identity.
The project's seats use it in their `.gitconfig`, and the merge queue uses it as both author and committer for merges it creates.
Projects that omit `git_author` and seats without a project use the required `[git] author`.

### backup

In `mork.toml`, `backup = "origin"` in a `[projects.<name>.forge]` table with `kind = "local"` names a remote in the project's `repo` for a project without pull requests.
After each local landing the engine pushes the current `refs/heads/<base>` to the same branch on that remote, never forced.
The local base stays the base of record; no PR, API or token is used.
Without `backup`, or on a hosted project, nothing changes.

The push uses SSH's BatchMode and a two-minute time limit under the project's outward-action lock, so its next landing may wait for that attempt.
Failure leaves the card done and the queue free once the attempt ends, with a held action and a red ask carrying git's words; `action.retry` runs it again.
Scheduling another backup clears no failure; a successful push drops the same project's earlier held backups and closes their asks.
A push interrupted by a restart runs again.

### toolchains

In `mork.toml`, `[mork] toolchains` lists folders shown read-only in every wall; `[projects.<name>] toolchains` adds that project's folders after the global ones.
An entry has `path` and optional `bin`, a folder inside it added to `PATH`.
These entries also list ordinary folders inside the operator's otherwise hidden home.
For agy, put the executable in a dedicated directory and list that directory in the global `toolchains` with `bin = "."`.

An entry's optional `hide = ["ACCESS*", ".env"]` lists names shown empty and read-only beneath that folder, including every alias such as `/data`.
The default is no hidden names.
Names match whole, case-sensitive; `*` matches any run of characters.
Each wall's start scans below the listed folder without following links, skipping `node_modules`, `.git` and `target` folders.
Matching directories are emptied as a whole.

### domains and host_ports

In `mork.toml`, `[mork] domains` lists destinations for all walled agent turns.
A card also uses `[projects.<name>] domains`.
List exact host names such as `"registry.npmjs.org"` for port 443 or `"registry.example:8443"` for one other port.
Names are case-insensitive.
Enclose IPv6 literals in brackets.
Both lists default to empty.
Configure the hosts for the model APIs and package registries your seats use before starting their turns.

`[projects.<name>] host_ports` lists host loopback ports as numbers or inclusive ranges, for example `host_ports = [3306, "6300-6302"]`.
Each port must be at least 1024, including the first port in a range.
The list defaults to empty.
A card also gets its preview and mock ports.
A review of a card's branch or its PR also gets the approved mock port named in the round request.
When assigning these, Mork skips live cards' preview and mock ports, the configured web port and every local tunnel port.
With `web_port = 0` it skips the port assigned by the OS.
The bound web bridge port and every configured tunnel port are excluded even when listed.
Each bridged port appears at the same port on the turn's private loopback, alongside its own test servers.

The supervisor runs `mork wall-proxy <job folder>` for the lifetime of each walled turn with the lists saved in its job.
The proxy named by `HTTPS_PROXY` and `HTTP_PROXY` accepts CONNECT for a listed host and port.
It rejects other destinations before DNS and returns 403 for plain HTTP requests.
An address on the machine itself also requires a bridged port.
The proxy carries bytes in both directions once the tunnel is open.

### web_port and web_assets

In `mork.toml`, `[mork] web_port` enables the localhost HTTP and WebSocket bridge, bound on `127.0.0.1`.
Without `web_port`, no bridge starts; `web_port = 0` lets the OS pick an ephemeral port for tests.
Both settings are read when the engine starts and take effect on the next start or reload.

`[mork] web_assets` names an optional folder of built static web UI files served without a key on every `GET` or `HEAD` path other than `/ws`.
With `web_assets` unset or missing, every non-`/ws` path answers `404`; other HTTP methods answer `405`.
Files are served confined beneath `web_assets` with no symlinks followed, `/` maps to `index.html`, and an extensionless path or a `/cards/<id>` path (dotted ids included) naming no existing file falls back to `index.html`.
The repository's web pages are built by `cargo xtask web` into `web/dist`, which `web_assets` may name.

`/ws` belongs exclusively to the WebSocket relay, which opens one `mork.sock` connection per WebSocket and carries one protocol JSON line per text frame both ways, with the operator's authority.
Every upgrade must carry the 64-hex secret from `<home>/web.key` (created once at mode `0600`, masked in logs and outward checks, never written to any log) in `/ws?key=<hex>` or `Authorization: Bearer <hex>`, or it answers `401`.
An `Origin` header, when present, must be `http://` on `localhost`, `127.0.0.1` or `[::1]` (any port), else `403`; a non-GET method answers `405` and missing RFC 6455 upgrade headers answer `426`.

### context_cap

In `mork.toml`, `[mork] context_cap` sets the shared auto-compaction context cap in tokens for Claude and agy seats, defaulting to `200000`.
Valid values are `0` (disables the cap, omitting `--autocompact` on Claude and `AGY_CLI_AUTO_COMPACT_OVERRIDE` on agy) or `100000` through `1000000` inclusive.
When active, the cap is passed to Claude via `--autocompact` and to agy via `AGY_CLI_AUTO_COMPACT_OVERRIDE`, and is used as the effective context window denominator (`min(model_window, context_cap)` on Claude, `context_cap` on agy) in seat indicators, `/usage` contexts and prompt `Context size` lines.

### fresh_context_percent

In `mork.toml`, `[mork] fresh_context_percent` accepts whole percentages from `0` through `100` and defaults to `70`.
A full card’s foreman starts a fresh session when it takes a fix or “Join the base” request and its stored usage exceeds that percentage of the effective window shown under “Context size”.
At 70% of 200,000 tokens, 140,000 keeps the current session and 140,001 starts a fresh one.
Zero disables the percentage trigger.
Reported compaction independently triggers a fresh session at the next such request, even after intervening turns.
Claude and agy support the percentage check when both usage and window are known.
Claude, agy and walled Codex report compaction.

### caches and cache_files

In `[mork]`, `caches` lists directories and `cache_files` lists individual files to share read-write across seat walls.
Both lists default to empty.
Each path is relative to HOME and consists of one or more normal path components.
Use a single type for each path.
A listed directory already includes its descendants.
Mork keeps the shared copies under its own `caches/` folder.
A missing entry starts empty.
For Cargo and npm, use `caches = [".cargo/registry", ".npm/_cacache"]` and `cache_files = [".cargo/.package-cache", ".cargo/.package-cache-mutate"]` so Cargo processes use the same locks.

### skills

In `mork.toml`, `[mork] skills` names an optional absolute folder path (with no `.` or `..` component) whose regular files are merged with the built-in skills at startup into the same content-named folder mounted in every wall.
Symlinks and non-regular files inside it are skipped, and a missing folder uses the built-in skills alone.
At the top level, `.claude-plugin`, `hooks`, `commands`, `agents` and `.mcp.json` are reserved plugin paths.
Any other error opening the folder, a regular file or directory at a reserved path, or a file colliding with a built-in skill fails startup.

### tickets and review_tag

In `mork.toml`, `[projects.<name>.tickets]` with `kind = "azure-devops"` requires `api` (an HTTPS work-items API URL without a trailing slash), `page` (an HTTP or HTTPS ticket page URL prefix) and `token` (the absolute path to the token file).
Every wall masks all projects' ticket token files.
Log redaction and outward text checks use those same configured files.
Projects with `tickets` parse ticket numbers from card notes and PRs, format PR titles, branches, descriptions and room details from them, and require a `PR wording` note before publishing a PR with tickets.
`[projects.<name>.forge] review_tag` names a non-empty title substring that triggers automatic PR review into any destination branch on a hosted project with an eligible source branch.

### colleague_review

In `[projects.<name>]`, `colleague_review = "standard"` is the default for colleagues' PRs and branches.
It keeps the lighter topic rules and filters known ESLint style, copies and words findings from the checks report.
Set `colleague_review = "full"` to send every check finding and use the same topics as our own PRs for the round's tier, including Design and simplicity when applicable.
Full reviews enforce the repository's AGENTS.md, architecture and test style.
The author and stakeholders decide product behavior.
Reviewers report broken behavior and regressions.

### claude_accounts

In `mork.toml`, `[claude_accounts]` maps each saved account's name (`[A-Za-z0-9._-]`, not `.` or `..`, matching `<name>.claude.json` and `<name>.credentials.json` under `login-home/.claude/accounts/`) to its non-empty usage pool header.
Each saved account needs a distinct header.
The built-in pools reserve `codex`, `agy gemini` and `agy 3p`.
Saved accounts may use `claude max` or `claude team`.
Without `[claude_accounts]`, no saved accounts are read and `claude.account` refuses every name as unknown.

### tunnels

In `mork.toml`, each `[tunnels.<name>]` entry configures a window-owned SSH web forward shown in the window's `web tunnels` overlay in name order.
Each entry requires `alias` (an SSH alias, non-empty, not starting with `-`, with no whitespace), `port` (non-zero unique local port), `remote_port` (non-zero remote port) and `url` (`http://` or `https://` browser address).
With no `[tunnels.*]` configured, the overlay lists no rows.

## Commands

### search

Finds commands by their syntax and help text in this binary's reference.

Every query word must occur somewhere in the signature or full entry, in any order and regardless of case.
Give the words separately or as one quoted query.
Each result shows the exact signature and a short matching passage, in manual order.
A match in the signature or opening line shows the opening summary.
With `--json`, the same results form an array of objects with `signature` and `snippet` strings.
With no matches, plain text prints nothing and JSON prints `[]`.
An exact `--help` argument anywhere after `search` prints this entry.
Search runs locally, including inside a wall.

Example: `mork ctl search merge queue --json`

### watch

The views of places, as they change: a live stream, not a one-off read.

Answers `{"watch": {"places": [...]}}`, then one line per place, `{"event": {"view": <view>}}` (`{"event": {"screen": ...}}` for `screen`), sent again when what it says changed: a state change that leaves a place's view as it was, its `revision` apart, sends nothing.
A place is `main`, a card's room (`ex-550`), a full card's spec room (`ex-550/spec`) or challenge place (`ex-550/challenge`), which the window shows nowhere, a DM (`ex-550/foreman/dm`) or `screen`; names aren't checked.
Without `tail`, a view carries the place's whole conversation, within its line; with it, only its latest `tail` messages, and the earlier ones whose ask is still open (`pinned`).
By default a view is the conversation alone: `{"place", "revision", "messages", "earlier", "pinned"}`, with no header, agents, offers, plan, notes, sessions, outputs, `turns` or `lists`.
With `--full` (JSON: `"full": true`) it is the whole view, described below, which can be large.
The command line prints every line as it comes, reconnects when Mork restarts, and runs until stopped: each word is one place.
The web Card Room and Mork Console direct a typed `/watch` to `mork ctl watch` in a terminal so board and chat updates stay live.
Changes nothing.

With `--full`, each agent row of a card's room and DMs carries its `turn`: read for the views alone and kept in memory, never in the state, so no gate, job or turn waits on them.
`turn`: the agent's latest turn, its own job's only (a child's never shows in its foreman's row), as its stream told it: its steps, each with the text and thinking given before it (thinking only where the stream gave nonempty thinking or reasoning; a subagent's marked `sub`), and `after`, the text no step followed; never the reply.
A view keeps only its agents' latest steps: `cut` says some went, and a text's own `cut` that it was cut.
The card header's `about.seat_costs` gives each seat's current-run `agent`, `cost_cents`, `unknown` flag and four `tokens` sums, including seats already gone.
Each agent row also carries an optional `cache` (`running`, or `idle` with `cold_at_ms`), and a card's room view carries `summaries` (its card's and each carried card's) and `findings` (its review card's raised findings with their states, without proofs).
A schema 7 reply uses `hand.message` for its text and `hand.ask` to flag it as a question.
A message carrying an open ask names its ID in `open_ask`.
Each open hand links back through `seq` and `message_place` and carries the message's headline.
A `screen` event's `hands` carry `cold_at_ms` while their waiting seat's prompt cache is idle, `projects` carries each project's look-back state, and `top.resources` carries the machine's sampled CPU and RAM.

Example: `mork ctl watch main ex-550`, `mork ctl watch main --tail 50`; with turn traces, `mork ctl watch ex-550 --full`

### hook

A project's orchestrator's stream: what reaches the operator or `orchestrator` about that project's cards, as it happens.
Each configured project has its own orchestrator, `orchestrator:<project>` (`orchestrator:example.com`), and only it sends its project's hook, with `mork ctl --as orchestrator:<project> hook`, on `mork.sock`: the hook's project is the sender's.
`--as orchestrator:<project>` makes any command count as that orchestrator's, never as the operator's; it is refused on an agent's socket and for a project not configured.
An orchestrator may send what the operator may, on any project's cards, except what a command's own reference keeps for him alone.
What it does shows as `orchestrator:<project>`; its messages answer only asks given to it (`give`, or asks given automatically on a delegated or self-sustained card).

Answers `{"hook": {"project": <project>, "from": <seq>}}`, then one line per event of that project's stream, `{"event": {"orchestrator": {"seq": ..., "time": ..., "project": ..., "event": ...}}}`, in order: `seq` is one numbering shared by every project's stream, so a project's lines skip the others'.
A line goes on the stream of its card's own project, a review card's included, whichever section the window groups it in: a message in a card's room or a DM of it, and every ask of the card.
A message to `orchestrator:<project>` goes on that project's stream; anything with no project goes on Mork project's stream when `[mork] project` is set and on no stream when it is unset; a `review` line goes on the stream of the project it names, and `general` memory's on Mork project's when configured.
The events: `message` for every message to the operator, `orchestrator` or `orchestrator:<project>` in any place (`place`, `card`, `from`, `text`, and `request: true` only for a seat's or the engine's request addressed to an orchestrator), never the rooms' chatter; `ask-opened` and `ask-closed` for every ask as it opens and closes (`ask`, `place`, `card`, `from` its agent, `kind`, `text`, and `answers`, its buttons' words and commands); `given` for an ask given to the orchestrators; `opinion` for advice on an ask (`ask`, the ask's details as on `ask-opened`, and `text`, telling the orchestrator to give the operator its opinion in its own chat and answer nothing to the seat); `review` for a project's part of a section the operator sent to review (`section`, `memories` or `notes`; `target`, the project or `general`, absent on a line kept from before targets, which was the whole section; and `text`, asking it to review that part alone, clean it up and report to him what matters).
Every event is kept until its project's hook was written it: a hook first sends every event of its project after the last one written to that project's hook, then the new ones as they come, so none is lost between two hooks or across restarts (one may come twice after a crash: its `seq` tells).
A project's very first hook starts from then, but keeps its `given` lines for asks still open and given, `opinion` lines for asks still open and not given, and every `review` line: these still arrive, first.
While it is open the project's orchestrator is hooked, and main says so when it hooks and unhooks.
A second hook of the same project replaces the first, which gets `{"event": {"orchestrator": "replaced"}}` and is closed; another project's hook is never touched.
The command line prints every line, reconnects when Mork restarts, and stops when replaced.

**Waking**

A project with waking enabled has a line Mork writes for each ask given to the orchestrators (`ask given to you: ex-550/answer/2` when delegation is off, `ask given to you (delegated): ex-550/answer/2` at the first level, or `ask given to you (self-sustained): ex-550/answer/2` at the second), each `opinion` (its whole text, ending `Ask in <place>.`), each `review` of its stream, a section the operator sent to review (its whole text), each request a seat or the engine sends `orchestrator` (`request from ex-550/foreman in ex-550`: who and where, never its text; an update or a response wakes nobody), and each of its cards whose run ends (`ex-550 is done`, `ex-550 is closed`), a review card's excepted, whose end wakes nobody, held until the card settles: its seats' closing turns ended and its clone removed, its handoff copied first, a removal held on a failed copy keeping it waiting until it is retried and done, dropped or done by hand; the line then names the notes of the card still listed under the card's project, as `notes` lists them, by kind and count (`ex-550 is done, left a feedback and a handoff`, `ex-550 is closed, left 2 code smells`, lessons and proposals counted too), and is the plain line when there are none: Mork's notes a seat of another project's card filed are Mork's orchestrator's to find with `notes`, and wake no line; a card live again before it settles is not said to have ended.
When `[mork] project` is configured, its orchestrator alone is also woken by each finished reload, with the whole announcement the new build says in main once it is said there (`Reloaded to a09d55a (from 5bbc110). It brought:` and its list); a failed reload, or a reload with `[mork] project` unset, wakes nobody.
What was asked or said is read on the stream or with `history`.
Each entry is queued as its event is recorded or as its card settles.
`wakes.json` keeps the queues and the cards still settling across restarts and reloads.

A project's section in `mork.toml` can set `orchestrator_session = "<Claude Code session name>"` or `orchestrator_waits = true`.
The two keys cannot appear together, even with `orchestrator_waits = false`.
A project with neither key has no wakes.
`hook` keeps its stream and cursor in either mode.

With `orchestrator_session`, a one-turn haiku run (`claude-haiku-5-5`, low effort, no session kept, no place in the window) sends up to 50 queued entries to the named session with SendMessage.
One run per project starts at a time.
Entries arriving during it wait for the next run, and a failed run is logged without retry.
Every run, its entries and its transcript included, is kept in `logs/wakes.log`.
A reload waits for no run: each in flight is cut, logged in `wakes.log`, and its lines go back at the front of their queue for the next build.
The haiku runs in a home of its own, `waker-home` in Mork home, in no seat's wall, with no user, project or local settings, no hook and no CLAUDE.md read, and a Claude login of its own, which the operator makes once with `HOME=<mork home>/waker-home claude` and `/login`; its wall uses the waker's own login.
With no such login (`waker-home/.claude/.credentials.json`) no run starts, the engine log says so once until it appears, and the queued lines are dropped, as a failed run's are.

With `orchestrator_waits = true`, an Antigravity Desktop orchestrator runs `wakes` as its daemon command to wait for the next batch.
This uses the command's connection alone, without a haiku run or a `waker-home` login.

Example: `mork ctl --as orchestrator:example.com hook`

### wakes

Waits for the sending orchestrator's project to have entries queued under `orchestrator_waits = true` (see Waking under `hook`).
Once available, it answers with `{"wakes": [<entries>]}` and ends the connection.
The command prints the entries verbatim to stdout, each followed by a newline, and exits 0.
A reload announcement keeps its embedded newlines.
There is no limit on the number of entries queued or carried in the answer.

The batch stays queued until the entire answer is written to the socket.
A failed write leaves it queued for the next waiter.
If the engine stops after the write and before recording it, the batch can be sent again.
If the daemon exits after the reply reaches its socket and before it prints the entries, that batch is lost.
The command reconnects after a restart or reload and keeps trying through downtime.

A second `wakes` for the same project replaces a waiter whose answer has yet to be sent to the writer.
The first prints the replacement error on stderr and exits 1.
If a batch is already being written, the second waits for that write to settle.
It receives the batch if the write fails.

Example: `mork ctl --as orchestrator:mork wakes`

### history

A page of a place's messages before one: what a view's `tail` left out.
A full card's challenge place, `<card>/challenge`, which the window shows nowhere, is read so.

Answers `{"history": {"place", "messages": [...], "earlier": n}}`: up to `count` of the place's messages just before the one numbered `before`, oldest first, as a view shows them, and how many came before them.
A `before` past the newest message (`999999`) reads the latest page; `0` reads none.
Messages alone by default; with `--turns` (JSON: `"turns": true`) the page also carries its `turns`, each retained turn between its messages, as the window asks for them.
A page past its line keeps the messages nearest `before` (`earlier` counts the rest), so asking again from its oldest reaches them; messages posted since move no page.
A read: allowed during a reload.
Changes nothing.

Example: `mork ctl history main 4120 50`; with the turns, `mork ctl history ex-550 4120 50 --turns`

### history.search

Finds the posted messages of cards' rooms and DMs and the cards' records containing any query word as a case-insensitive substring: those matching the most words first, then a card's text and notes, then the newest, at most 20.
A message is searched by its headline, its text, any ask options and recommendation and each of its findings' file, line, summary and proof.
A card's records are its title and text, each of its dated notes, and its journal events but its messages'; turns' transcripts are not searched.
A card on a board is read as the engine last read its file, writes made after it ended included, its notes told apart as `card.show` tells them; one on no board as it was stored.

Each hit names its `card` and `source`: `message` with its `place` and `seq` (`history <place> <seq + 1> 1` reads it), `note` with its `number` as `card.show` gives it, `card`, or `journal` with its `kind`; its `time` (none on a note or a card's text), its `author` (a note's signature, the engine's journal, none on a card's text) and an `excerpt` around its first match.
`marked` says only what the records say: `standing decision` or `withdrawn decision` on a marked decision note, `delegated answer to <ask>` on an orchestrator's message the engine marked as answering an ask on the operator's delegation; anything else, ordinary chat and agreements nobody recorded included, is unmarked.

A seat searches its own card's project; an orchestrator or the operator every project.
It reads the state on a connection of its own, holding up no other command.
Answers `{"history.search": {"hits": [{"card", "source", "time", "author", "marked", "excerpt", …}]}}`.
Refused for a seat of no card.
A read: allowed during a reload.
Changes nothing.

Example: `mork ctl history.search "preview port"`

### history.journal

Reads a card's journal around one event, as recorded, without rebuilding its state or reading job folders.
Name at most one of `--seq <seq>`, `--at <epoch milliseconds>` or `--before <seq>`, and a positive `count`; with none, it reads the newest `count` rows, `later` 0.
Journal seqs are insertion positions, distinct from message seqs; rows are always returned in seq order, oldest first, even when their times repeat or go backwards.

With `--seq`, up to half the count (rounded down) precedes that journal position and the rest starts there.
With `--at`, the position is the first matching row whose time is at least that time; if none matches, it follows the newest row.
`--before` returns up to `count` rows strictly before the position: repeat it with the oldest returned seq to page backwards.
`--kinds ask,reply` limits the rows and the page counts to those exact kinds; without it, all kinds match.

Answers `{"history.journal": {"card", "rows": [{"seq", "time", "kind", "data", "message"?}], "earlier", "later"}}`.
Each row keeps its journal data intact.
A `message` row's `message` is the stored message named by `data.seq`; a `reply` row's is the stored hand message named by `data.hand`, including its hand and findings.
A missing retained message leaves the attachment absent.
Attachments use history's text limits and carry `cut` when shortened.
The page has history's byte bound, retaining the rows nearest the position; `earlier` and `later` count all matching rows omitted on either side, including those left out to fit.
An indivisible row too large for a page is refused.
This is a count-limited page, never an unbounded journal dump.

Rows recorded without a card are included when they name the card's seats, places or exact stored job ids.
A `limits` row belongs to the card that owns the job named in `data.read.job`.
Closing memory edits appear under the issuing seat's card as `memory.write`, `memory.delete` or `memory.skipped`.
A `memory.staged` row carries only the seat and operation.
These kinds locate the recorded facts; their data records changes, not a reconstructed snapshot:

- `message` and `reply`: posted words, hands and accepted reply outcomes; `reply.refused`: a rejected reply's reason, without its rejected hand.
- `ask`: an ask's id, kind and open flag; `give` and `opinion`: delegation and consultation.
  These rows do not retain every field of the ask.
- `child.spawned`, `review.released`, `closing.owed`, `closing.answered`, `closing.resumed` and `closing.ended`: child and seat lifecycle marks; an accepted closing reply is a `closing.answered` row carrying `agent`, `feedback` and `mork_feedback` (booleans, never a `reply` row; `mork_feedback` is false when `[mork] project` is unset), and `closing.resumed` marks a closing superseded by a reopen.
- `job.intent`, `job.running` and `job.settled`: jobs prepared, started and settled; `deliver`, `interrupt` and `stop`: recorded delivery and interruption marks.
  Turns owed are not recorded.
- `cost`: a turn's `agent`, `session`, `cli`, `model`, `effort` and `at`; its dollar `delta` and session `total` in millionths of a dollar, or `unknown: true` and zero `delta` when dollars are unknown.
  When usage was reported, `tokens` holds the turn's disjoint `input` (uncached), `cache_read`, `cache_write` and `output` shares; output includes reasoning.
  Claude includes subagents.
  Codex app-server includes every reported thread, children priced at the turn's own model; `codex exec` reports only its parent thread.
  Codex and agy dollars estimate standard API list prices.
  Subscription quota is reported separately.
  Older lines retain their dollars and contribute no tokens; a running session first seen by this accounting contributes its whole token total on that next line.
- `action`, `action.started`, `action.done` and `action.dropped`: outward work requested, started, completed or dropped; `card.stage`: stage and pause changes, checks included.
- `entry.entered`, `entry.moved`, `entry.finished`, `entry.conflict`, `merge.queued`, `merge.slot`, `merge.tested`, `merge.left` and `merge.held`: candidate and queue work.
  Check and suite job lifecycle marks appear among the job rows.

A seat reads cards of its own card's project; when `[mork] project` is set, a seat of a card of that project reads any project's cards.
The operator and the orchestrators read any.
Refused for a seat of no card, a card outside its reach or an unknown card.
The read runs on its own read-only connection and one transaction, ended cards included, holding up no other command.
Allowed during a reload.
Changes nothing.

Example: the newest rows, `mork ctl history.journal ex-550 40`; around an event, `mork ctl history.journal ex-550 40 --seq 12000`; by a message's `time_ms`: `mork ctl history.journal ex-550 40 --at 1794351522350 --kinds message,reply,ask,job.settled`; an older page: `mork ctl history.journal ex-550 40 --before 11980`

### status

The live cards, one line each; job records only when requested, the live ones by default.

Answers `{"status": {"lines": ["ex-550  brief  yellow  asks 1  Its title"], "revision": n}}`; a run that carries cards names them at its line's end (`  carries ex-551 ex-552`).
With `--jobs` (JSON: `{"status":{"jobs":true}}`), also includes `jobs`: a list of `{"id", "kind", "settled", "settled_ms"}` records for the jobs not yet settled.
`--all` adds the settled jobs whose folders are still kept, which build wait and `doctor` read.
`--card <card>` keeps the jobs of that card alone: a turn of its agents, or a work job (clone, install, bundle, check, candidate, train, suite) made for it; other cards' and Mork's own builds are left out.
Both need `--jobs`.
A read: allowed during a reload.

Example: `mork ctl status`; with job records, `mork ctl status --jobs`; one card's, `mork ctl status --jobs --card ex-550`

### usage

Every CLI's usage-limit windows, each agent's context use, and the rolling 7-day burn summary.

Answers `{"usage": {"by_pool": ["codex week: 73% used, 85% of the week gone: ends near 86%"], "limits": [{"cli", "account", "bucket", "window", "name", "percent", "resets_at", "reached", "seen_ms", "length_ms", "source", "age_ms", "unknown"}], "contexts": [{"agent", "used", "window", "model"}], "recent_burn": {"since_ms", "total_cents", "unknown", "roles": [{"role", "cents", "unknown"}], "ended_cards", "average_cents", "cache_by_model": [{"cli", "model", "cache_hit_percent"}], "cards_left": {"pool", "cards"}}}}`.
`unknown` says why a window has no share.
A Claude `account` holds its `id` and plan (`team` or `max`).
It is null for Codex and agy.
`contexts` lists every Claude and agy seat in the state that has a reported context size, taking its live stream context first and falling back to its stored context and effective `context_cap` window after a reload.
`recent_burn` reads the journal only when `usage` is asked: rolling 7-day role burn (seat name with `/dm` and trailing `-<n>` dropped, largest first), whole-run average over runs ended in the period (with folded internal review cards counted inside their parent), prompt cache hit rates by CLI and launch model, and `cards_left` for the tightest live weekly pool (matching agy cost lines by model bucket, skipping pools with no Mork cards in their week, and reporting `0` when a pool is reached or full); optional fields are omitted when unknown.

`cache_by_model` lists each recorded CLI and launch model pair in that order.
Each percentage divides the pair’s total cache reads by its total prompt tokens (uncached input, cache reads and cache writes), rounded to the nearest whole percent.
A pair with no measured prompt tokens omits `cache_hit_percent`.
A measured zero is `0`.

Claude readings appear in `by_pool`, the full `limits` fields and the window's Usage section only for the live account or a saved account.
Readings for other Claude accounts stay stored and show again if the account becomes live or saved.

`by_pool` gives each pool's pace, one line each, above the full fields in `/usage` and first in the answer (its keys print sorted): Claude's week, Fable and any 5-hour pool for each account, Codex's week, and agy's five-hour and weekly `agy gemini` (Gemini Models) and `agy 3p` (Claude and GPT models) pools.
While agy's token file exists, the engine reads both groups every 60 seconds with `agy --output-format json -p="/usage"` in a wall.
The read starts no conversation and spends no quota.
Each saved account's pool shows under the header configured for it in `[claude_accounts]` (for example, `claude max` for `account-max`).
Pace uses the reading's time: used share divided by the share of its window then gone, rounded to whole percentages.
Missing shares or reset times, or passed resets, have no pace.
A passed reset shows as `stale`.
A signed-out saved Claude account keeps its figures with `(read 2d ago)`, until a new turn reads that account again.
The live account is the one in `login-home/.claude.json`, whether or not it is a saved account.
In the right column's Usage section, each pool's windows show their used share, projected end after an arrow, red at 100% or over, and time left; the signed-out account is dimmed with its reading's age on its pool line.
Its header is clickable when it belongs to a saved account: it sends `claude.account` and a successful switch swaps the headers' tones immediately; a refusal shows in the note line.
An account with no reading yet has no header and is reached through the command alone.
Fable resets with the week and has no separate countdown.

Example: `mork ctl usage`

### claude.account

Switches Claude seats to the saved account `name` configured in `[claude_accounts]`.
The operator and the orchestrators may send it; every seat is refused.
The orchestrators' and the waker's own logins are untouched.

It matches the login home's `oauthAccount` to its saved account and saves the active `logins/claude/.credentials.json` and the account fields in `login-home/.claude/accounts/`.
It then replaces the active credentials and sets `oauthAccount` and `userID` in `login-home/.claude.json`.
Each Claude seat receives those fields at launch.
Other configuration fields stay as they are.
Each file is written whole with its own mode.
No token is logged or answered.

Answers `{"claude.account": {"account", "replaced"}}`, the saved account now in place and the one it replaced.
Refused with nothing written when the name is not a saved account, its files cannot be read, that account is already active, or the active Claude login matches no saved account.

Turns started after the switch use the new account.
A turn already running may move to it; from a switch on, the engine ignores usage readings from turns that started before it, including after a switch back.

Example: `mork ctl claude.account account-team`

### projects

The configured projects, in the order their tables appear in `mork.toml`.

Each entry in `projects.projects` has `name`, `base`, `prefix` (the board's card ID prefix), `local`, `forge` and `auto.on`.
The `passes` rows give each read's mode, last completion and running card.
Scheduled rows include thresholds.
Auto rows give the live `score` parts and `total`, and the current `bar`, or `null` when quota prevents a start.
A local project has `local: true` and `forge: {"kind":"local"}`.
A Bitbucket project has `local: false` and its `forge` holds `kind`, `workspace` and `slug`.
A GitHub project also has `local: false` and exposes `kind`, `owner` and `repo`.
A read.

Example: `mork ctl projects`

### lookback

`mork ctl lookback <project> [--read pulse|deep-sweep|dream]` starts a report.
The default is `pulse`.
Only the operator may start a pass by command.
The command refuses an unknown read or project, or a project with a live look-back.

Answers `{"lookback": {"card"}}`.
The engine starts the report on a new card, `lookback-<project>-<n>`, with a fixed commit and a file of inputs named in its description.
Pulse covers commits since its last run, ended cards since that run started, and the instructions and open cards touched by the landed changes.
With no saved commit, Pulse looks for the last recorded base tip before the seven-day cutoff; the skill gives the fallback if that tip is unavailable.
With no ended-card cursor, Pulse reads the past seven days of ends.
Deep Sweep reads the whole code at the fixed commit for dead code, dead tests, risks and wording.
Both reads receive the open board cards and group their findings into proposal notes.
Dream verifies and consolidates project memory at the fixed commit and folds handled lesson notes.

Scheduled passes start when any threshold is met.
Landings count landed cards, edits count committed project memory writes and deletes, and notes count outstanding project lessons filed since the baseline.
Days count from the last baseline.
On first configuration, counts start at the first check and days start from the last completed read if there is one.
Completion records the run's starting counts and the end time.
Dream excludes its own edits from the next trigger.
Ending an engine-started run early with `done` or `close` resets the thresholds the same way and preserves the previous completed read's date and range.

Auto scores each signal since the baseline as `100 * x / (x + half)` and adds the parts.
Pulse uses halves of 20 landings and 7 days.
Deep Sweep uses 200 landings and 30 days.
Dream uses 20 memory edits, 5 lesson notes and 14 days.
Pulse and Deep Sweep need a new landing.
Dream needs a new edit or lesson.

For each allowed pair in `picks.pass`, the engine uses the quota pool available to its seat, including only the signed-in account on Claude.
The weekly spare is the percent projected unused at reset from the reading, multiplied by the fraction of that window now elapsed.
The smallest applicable weekly spare counts.
A reached current window or a five-hour share above 60% closes the pair.
A missing five-hour share permits a run unless the limit is reached.
A required weekly reading must be usable and, on Codex and agy, at most ten minutes old.
A fresh reading on those CLIs showing zero use with no future reset counts as 50 spare.
An expired Claude week needs a new reading.
The pair's score is `reasoning index * spare / (spare + scaled cost)`, using benchmark dollars per task scaled by 3.5 for Claude and agy third-party models, 13 for Codex and 2 for agy Gemini.
Ties follow list order.
The winning pair's spare must be positive to start a pass.
The bar is 100 minus that spare.
The pass furthest above its bar starts, with ties in project then pass order.
One auto pass can be live across all projects.
While a real card has work running or owed, including a closing turn waiting for clone removal, auto starts no new turn.
Running turns finish.
A scheduled or auto run records its cursor and ends at `report-ready`.

For a manually started pass, `done <card>` records the last run for its read once the report is ready, advancing its cursor to its fixed commit.
An earlier `done` or `close` ends the card without recording a run.
Each Projects row shows what the read does, when to use it, the last run's date and age and, when known, the number of commits since its fixed commit.
During a run its card replaces the row's button and the other reads' buttons are hidden for that project.

Examples are `mork ctl lookback mork` and `mork ctl lookback mork --read deep-sweep`.

### build

Builds Mork's own binary from a commit of its repository, in a wall.

`commit` is anything git resolves (a hash, a branch, a tag).
The build runs as a job: `cargo build --release --locked`, then the binary and its manifest go to `builds/<commit>/`.
It never reloads.
A build of a commit on the base branch past the pin is pinned, checked as `pin` checks it, so an older build never moves the pin back.
A failed build is said to the operator in main.
Answers `{"build": {"job", "commit"}}`, the commit resolved.
Refused when `[mork] project` is unset or not a configured project, the commit isn't one of its repository, or the source can't be extracted.

For the orchestrators: Mork updates itself the same way: its base branch's tip, read in the operator's checkout every 20 s and at each start, is built when it is past the pin (with nothing pinned, past the running build), and the build pinned; a build made before its commit reached the base is pinned at the read that finds it there.
The top line says the update is ready, and its `reload` button, a `reload`, or a restart runs it.
A failed build, or a refused pin, is said in main, the pin left where it was, and waits for a retry: the top line's `retry`, or `build` or `pin` again.
A commit once pinned is never pinned by itself again, so a pin moved back by hand stays.

Example: `mork ctl build 3f2a9c1`

### pin

Moves the pin: the build Mork runs at its next start, or reloads to.

Answers `{"pin": {"commit"}}`.
The move is logged in `pins.log` with who made it; nothing reloads.
Refused when `[mork] project` is unset, there is no build of the commit, it isn't on Mork repository's base branch (a pin is a commit the operator merged), the build can't read the state's version, or an unsettled job has a format or reply schema the build doesn't read.

For the orchestrators: every pin, manual or automatic, also points `~/.local/bin/mork` at the pinned build.
A completed reload does the same for its target, so terminal commands follow the selected version without another `install --path`.
As with installation, an existing file that is not a link to a Mork build is not replaced: the pin or reload is refused before anything moves.
The existing pin and reload records finish an interrupted shortcut update at the next start.

Example: `mork ctl pin 3f2a9c1`

### reload

Switches the running mork to another build without stopping it: the given `target`, or the pin.

Before the hold, the target build reads and validates the live `mork.toml` using its startup parser.
If the file fails validation or the check cannot run, including on older builds that lack it, `reload` returns the error and the current build keeps running.

After a short hold, and up to 10 s more for outward work in flight, the engine executes the target build in its place.
The new build keeps the sockets and takes over running turns.
It moves the pin to itself and says "Reloaded to … (from …)" and what it brought in main, the same text a wake line for Mork project's orchestrator when `[mork] project` is set (Waking, above).
It announces changes to the Mork manual and the reply contract in every place with a running session.
It addresses a role manual's change to that role in rooms and DMs with a running session of that role.
The announcements wake nobody and stay in room and DM history.

Only resumed seats of the matching role read a role manual announcement in their turn news, including planners after approval.
This also applies to unread announcements from earlier builds.
Manual changes arrive as a diff with context, or a short notice pointing to `mork manual <name>` when the previous text cannot be recovered.
The reply contract arrives whole.
New sessions receive complete manuals.

A process restart or reload doesn't end agent sessions.
Answers `{"reload": {"target", "id"}}`.
Refused when a reload is already under way, nothing is pinned and no target is given, `[mork] project` is unset (with or without a target), or for any of `pin`'s reasons.
A failed reload is said in main, and the old build carries on.

Example: `mork ctl reload`

### say

Posts a message to an agent in a place, and wakes it, or hands it to the agent's running turn.

The place is `main`, a card's room `<card>` (one of its agents with `--to <name>`), a full card's spec room `<card>/spec`, or a DM `<card>/<name>/dm`.
A refusal for an unknown place lists these forms.
A card's challenge place, `<card>/challenge`, is refused: no one says anything there.

The agent is `to` when given, else a leading `@name` in the text, else the place's own: a card's room addresses its foreman at every stage, and a spec room its planner.
Each room has one seat: naming `planner` in a card's room, or `foreman` in its spec room, is refused, saying which room it sits in.
A DM addresses its own seat.
Main has no agent of its own: the operator's or an orchestrator's words there with no `to` nor `@name` are posted and wake nobody (`{"say": {"seq", "to": null}}`).
An agent reaches the operator only by raising its hand: its plain words in main, and a `to` or `@name` of `operator`, `room`, `runner` or `mork`, are refused, saying so.
The message is a request: the agent is owed a turn, an error, cut or stopped agent is retried, and a paused card's agents wait for its `resume`.
Your images must be in your seat's `io/` folder (`MORK_IO`), as a reply's `images` must be: each is copied as you send the message, which carries that copy whatever the folder holds later, and one that can't be copied refuses it.
From the operator, it answers the agent's open `answer`, `blocked` and `error` asks; from an orchestrator, only those given to it (`give`, or auto-given on a delegated card), whatever the message's `kind`: any other ask stays open.
From the operator, in its own room, a recap waiting from the agent holding the specification is withdrawn.
From the card's own project's orchestrator, in the specification holder's own room while that card's open `approve-brief` ask is given to it (never on a visual card) and it is not answering the holder's unanswered mechanical request, its message withdraws the waiting recap, closes that plan ask and is marked as answering it on the operator's delegation; any other orchestrator message (an ungiven plan ask, a visual card's, another project's orchestrator, or an answer to an unanswered mechanical request) leaves a standing recap as it was.
A message answers no engine question: words in a room, a plain yes included, never approve.
A delegated plan never approves itself: every waiting plan and mock needs `approve`.
The operator may approve either; the card's project's orchestrator may approve only a non-visual plan whose open `approve-brief` ask is given to it.
`kind` is the message's, `request`, `response` or `update`, as a reply's message carries it: an agent's `say` must give it, and is buzzed without it; the operator's needs none.
It is kept with the message, whose news line says its verb.
`delivery` `now` (the window's Send now) cuts the agent's running turn: what it ran and edited stands, its reply is dropped, and once that turn has settled its session continues with the message, the prompt first saying the turn was interrupted and to check what already happened; the card and its other agents run on.
An agent with no turn running, or paused, has it as an ordinary message.
`choice`, the operator's or an orchestrator's alone, names by its number the option of the addressed agent's open `answer` ask that the message chooses (the window's option buttons send it): it is kept with the message, and a number that ask doesn't offer, a `say` with none open, or an orchestrator's on an ask not given to it, is refused.
Without `delivery`, a message from the operator, an orchestrator, or the agent's parent seat, as a parent's reply to its child, reaches the agent's running turn between its tool calls and wakes no turn of its own: a Claude turn has it staged for its next model call, a walled Codex turn steered with it.
The engine's report of a child's ended turn (finished its task, passed or failed its topic, blocked, asking or a red hand) likewise reaches that child's parent's running turn, whatever it says.
Messages a child writes to its parent in a reply or with `say` stay queued for the parent's next turn.
The turn's stream says whether the model read it: read by a turn whose reply applied, it is settled; otherwise (the turn ended first, or failed, was cut, or its reply was refused) the agent is woken, its next turn's news giving the message again, marked as perhaps already in its session.
Messages to an agent with no turn running, a paused or retiring agent, an unwalled Codex turn or an agy turn wait for the agent's next turn, as `queued` does.
Other messages outside the live delivery cases wait too.
Only the operator, an orchestrator, or the agent's parent seat sends one now: another seat's is buzzed.
A child can address its parent from its own DM.
Its message posts in its parent's room and reaches that parent.
On a full card with a planner, the foreman's `planner` and the planner's `foreman`, by `say` or a reply's message, from its room or the foreman's DM, and either seat's unnamed words said in the other's room or the foreman's DM, post in the card's challenge place and reach the other seat there, at every stage.
The planner answers the foreman's DM by `foreman/dm` (`<card>/foreman/dm`): its words reach that seat alone, never the room's foreman.
The window shows that place nowhere; the step of such a `say`, or of one whose place or recipient its command line doesn't show, a shell line it wraps (`bash -c '…'`, `eval '…'`) read as its own, says only that a message was sent, and `history` reads it.
The operator and an orchestrator reach the planner in the spec room at every stage.
Opening the planner's ask in the window opens the spec room.
A card started with `--no-planner` has its foreman in its one room from the start, and a `planner` address there is refused: it has none.
Answers `{"say": {"seq", "to"}}`, `to` the agent's key.
In a card's room, a `to` (or leading `@name`) naming one of the card's children (`researcher-1`, `reviewer-1`) posts in that child's DM, `<card>/<name>/dm`, and wakes it; a planner's researcher is named from the spec room.
Refused when its `text` is empty or only whitespace and it carries no image, or when no such agent is in the place.

Example: `mork ctl say ex-550/researcher-1/dm --kind request Where is the empty cart tested?`

### message.send-now

Interrupts the recipient of an unread message so its session continues with the pending messages already posted.

Give the original message’s `place` and `seq`.
The operator, an orchestrator, or the recipient’s parent seat may interrupt it.
The command checks the recipient’s latest read receipt and refuses if the message has been read or the turn can no longer be interrupted.
It returns `{"message.send-now": {"seq", "to"}}` and keeps the original message in the conversation.

Example: `mork ctl message.send-now ex-550 412`

### dm

Opens a DM between the operator and one of a card's agents: its foreman.

The DM is a place of its own, `<card>/<role>/dm`, with an agent and a session of its own; nothing is posted and no one woken.
In the window it has no row: its card's tabs, after its spec and build rooms' on a full card with a planner, have one for each of its DMs, and one for its foreman's before it is opened, which opens it as `dm` does.
The foreman's DM agent names `planner` to reach it in the challenge place.
Answers `{"dm": {"place"}}`, the same for a DM already open.
Refused for other roles (a foreman's children have theirs from `spawn`), an unknown card, or a role the card has no agent in.

Example: `mork ctl dm ex-550 foreman`

### spawn

Spawns a researcher, reviewer or baseline seat for a card's foreman: a child, named, walled, in a DM of its own.

Researchers require `model` and `effort` except on an auto-started pass.
There, omit `model`, `effort` and `cli`.
The engine chooses a pair from `picks.pass` at spawn and refuses the spawn if no quota pool is open.
An open pool with measured zero spare can serve a researcher.
A reviewer requires `topic`, what it reviews in the foreman's own words, quoted so a topic of several words passes whole (`--topic 'money, permissions or data integrity'`): the window's room list shows it beside the reviewer's name, and it is saved on the seat only, never on the card.
A researcher that names one is refused.
A reviewer may omit `model`, `effort` and `cli` together to launch on its configured inside or outside default, without allowlist permission.
A baseline seat names none of them: it launches on `[seats.baseline]`.
If any selection option is supplied, both `model` and `effort` are required.
An explicit selection chooses the child before its first turn.
`model` is a full id, or the first word of a listed label (`sol` or `opus`, in any case) when no id matches it.
A refusal names the ids when several models share that word.
When the model name occurs on more than one CLI and `--cli` is omitted, a known model takes its own CLI (`claude-opus-5-5` / `opus` stays Claude); name `--cli` (`claude`, `codex` or `agy`) to pick another CLI or when the model has no default CLI.
An explicit selection must match one entry in `[[picks.researcher]]` for a researcher, or `[[picks.reviewer.inside]]` for an inside review and `[[picks.reviewer.outside]]` for an outside review, in `mork.toml`.
Each entry has `cli`, `model`, `label` and its own `efforts`; the allowed effort belongs to that CLI and model.
An absent or empty list permits no foreman-selected model for that role or review kind, for spawn and later picks alike.
Explicit selections never inherit permissions from the other review kind, seat defaults or the code's catalog.
The entries supply the model and effort permissions, and an explicit selection takes high or xhigh alone, whatever efforts its entry lists.
Selections preserve the seat's wall: an unwalled Claude seat runs Claude models only, because its deny list exists only in Claude Code.
An agy selection requires a walled seat.
Automatic seat defaults and the operator's own picks are independent of these permissions.
A refusal creates no child and wakes nobody.

A refusal names the permission list and its permitted models or, for a refused effort, the permitted efforts for that CLI and model.
For reviewers it names `picks.reviewer.inside` or `picks.reviewer.outside`, as does the review foreman's Allowed models section, showing only that kind's permissions.

This example allows Sol at high or xhigh for researchers and outside reviewers, and Sol at xhigh for inside reviewers.

```toml
[[picks.researcher]]
cli = "codex"
model = "gpt-6.2-sol"
label = "Sol"
efforts = ["high", "xhigh"]

[[picks.reviewer.inside]]
cli = "codex"
model = "gpt-6.2-sol"
label = "Sol"
efforts = ["xhigh"]

[[picks.reviewer.outside]]
cli = "codex"
model = "gpt-6.2-sol"
label = "Sol"
efforts = ["high", "xhigh"]
```

Researcher and reviewer pick lists grant a foreman no permission to select Fable, Astra or `max`.
For auto pass foremen and researchers, the engine can select Fable, Astra and supported `max` efforts explicitly listed in `[[picks.pass]]`.

Each additional model has its own entry.
A plain `[[picks.reviewer]]` is rejected as a config error naming its line, with no compatibility reading.
Configure every role and review kind the foreman needs to select explicitly; an omitted or empty list grants no choices.

The child is `<card>/<role>-<n>` (`ex-550/researcher-2`), numbered after the last of its role, its parent the seat that spawned it.
Its place is its DM, `<card>/<role>-<n>/dm`, nested under its card's row in the window, which folds it, where `task` is posted from its parent as a request that wakes it.
It has a session of its own, which its later tasks resume: the foreman's message to it by name (`researcher-1`), or the operator's in its DM.
The engine reports its `built`, `reported` or `blocked` to its parent in its parent's room (the spec room for a planner's researcher), reaching its running turn between tool calls or waking it for its next turn: it submits nothing, the foreman alone submits the card with its own `built`, after merging `origin/<base>` in that turn.
Between a foreman's turns, a question, `blocked`, or a red hand outside a reviewer's verdict wakes it at once.
A plain completion or a reviewer's verdict with `ask` false, reaching a foreman between its turns with no wake owed it, while another of its children is at work (its turn running, or one owed it that may start now: unpaused, any usage limit reset) is held: the held reports wake the foreman once, carrying their round, when its last child at work stops by any end, and stay held until a turn it launches with its news reads them.
A retry or correction keeps them.
Resuming the foreman restores a wake that a stop or second cut dropped, after the replay if one is owed.
A planner's researchers wake it per report.
A submitted head lacking the newest base tip the engine has seen gets a join request, the tip imported first, before the checks run in its clone.
A researcher works in a folder of its own, the clone read-only at `$MORK_CLONE`.
A visual card's baseline seat (`ex-550/baseline-1`) is its foreman's at the card's baseline step alone, one at a time: it works in the clone, reading it and writing only its `.git/mork-mock/`, and its `built` or `blocked` reports to the foreman, whose `built` ends the step and releases it.
A review card's reviewer (`pr-ex-12/reviewer-1`) is one of its blind reviewers, one per topic, with its topic in the first task in its DM.
Its `review-pass`, `review-fail` or `blocked` reports to the foreman with its findings and decides nothing.
A verdict the foreman gives for its round drops a wake caused only by queued reports, which stay in the room's history.
A report delivered into the foreman's running turn still wakes it if left unread.
The reviewer stays after a failed round to verify its findings' fixes (`resolve`) and review changes since its own last reviewed round, including the files from skipped rounds and the merged base; in later rounds Mork tells it its own earlier verdicts, reported findings, and kept findings' statuses.
It traces those changes for its topic into affected callers and dependents, including unlisted files.
A passed round releases it once it has no findings left to verify.
Answers `{"spawn": {"agent", "place"}}`.
The card's foreman may spawn children.
Its planner may spawn researchers.
Refused for a card that isn't live, a role other than `researcher`, `reviewer` or `baseline`, a baseline seat outside its visual card's baseline step, while the card has one that isn't retiring, or naming `model`, `effort` or `cli`, a reviewer on a card that isn't a review card, an empty task, a reviewer without a topic, or a researcher with one.

For example, `mork ctl spawn ex-550 researcher "Find where the cart's totals are computed" --model gpt-6.2-sol --effort xhigh`, or `mork ctl spawn pr-ex-12 reviewer 'Correctness: ...' --topic correctness`.

`--task-file <path>`, in `mork ctl`'s words alone, reads the task from that file, or from stdin for `-`, so no shell quoting touches it: its text goes as `task` verbatim, its trailing newlines trimmed.
The build Mork runs reads it once, after any handover to that build.
Refused with exit 2 and a usage line, sending nothing: a task in words or `--task` beside it, no path, an unreadable file, an empty text, or a request over Mork's 1 MiB line.

Example: `mork ctl spawn ex-550 researcher --task-file .git/task.md --model sol --effort xhigh`, or from stdin: `mork ctl spawn pr-ex-12 reviewer --task-file - --topic correctness < task.md`

### ack

Answers a `new-pr` or `answer` ask without acting on it, closes an alarm ask (not one holding an action: its retry or drop answers that), or closes an error ask of a card that has ended (done or closed).
An agent's `ack` of an alarm is buzzed: only the operator or an orchestrator closes one.

Answers `{"ack": {"ask"}}`.
Refused for another kind of ask, an alarm holding an action, or one not open.

Example: `mork ctl ack ex-550/answer/1`

### start

Starts a run of a board card: `full` (the default) or `report`.

`--delegated` starts the run at the first delegation level; `--self-sustained` at the second.
Either overrides the previous run's level and project auto, a lower level included, in full or report mode.
Both together are refused.
With neither, an enabled level from the previous run is kept; otherwise project auto starts it at the first level when on.
These flags are the operator's or an orchestrator's alone: any other seat's start selecting a level is buzzed.

Its earlier run's agents go; the card is fetched, cloned, installed, and its board status set to started.
Its first run's clone is on a new branch, the base's tip beside it as `origin/<base>` and the local `<base>`, which every import of the base moves too (a review round's clone has only `origin/<branch>`, and no local base beside a branch named like the base); a later run goes on with the card's branch at its head, its published head and PR kept, or from the base once an earlier run landed.
A full card's start seats its foreman in the card's build room, `<card>`, and its planner in its spec room, `<card>/spec`, the planner's parent the foreman.
Both wait for the clone.
The first turn goes to the planner, in the spec room, which scouts and settles the specification under the card's delegation level, and the foreman's first turn challenges the first recap, unless the operator's words in the build room wake it before.
On a visual card the first turn is the foreman's instead, asked to "Build the baseline", and the planner's turns wait until its `built` ends that step.
The planner records its decisions on the card and recaps the specification and code shape.
The foreman challenges each recap in the card's challenge place, `<card>/challenge`, before it is offered; the planner settles objections and recaps when needed.
Approval hands that foreman the build in its same session with the approved plan and recorded decisions, and releases the planner's brief researchers.
The foreman is asked to build; a visual card's mock, made at brief on its baseline, ends with that approve.
A report starts at its foreman's report request.
With `--visual`, a full card starts in visual mode, as `visual` sets it, its baseline step opening once the clone is made; a later full run keeps the mode its last run had, and a report run has none.
Answers `{"start": {"card"}}`.
Refused when no configured board holds it, `--visual` names a mode other than full, it is live, it is done on its board, its closed run's last head is still being kept (its final bundle, that head's import or its clone's removal is under way: start it again once the clone is removed), the mode is `light` or `review`, or, for full mode, the operator's review of its branch (`review.branch`) is under way: `done` it first.
With `--visual`, the operator's or an orchestrator's alone: any other seat's is buzzed.

`--model` and `--effort` select the seat that starts the card's work as `pick` does, except `start` has no `--cli` option and refuses Fable, Astra and `max` to every sender, the operator included.
That seat is the planner on a full card, a visual card's taking its first turn once the foreman's baseline step ends, and the foreman on a full card started with `--no-planner` or a report card.

`--no-planner` starts a full run with no planner: its foreman holds the specification at brief and after approval.
The first turn goes to the foreman, once the clone is made, and its wall is the foreman's.
On a visual no-planner card, "Build the baseline" comes first, and the foreman's `built` ends the baseline step and begins the specification conversation on the mock; otherwise "Agree the specification" comes first.
It scouts, may give researchers questions, raises genuine tradeoffs or departures in its flagged hand under the card's delegation level, records answered decisions, and recaps the specification and code shape, its sketch in the hand's `changes` and `code_shape`.
It ends `brief-ready`, an outcome offered to this foreman alone at brief past any baseline step and while a reopened mock is open.
Before approval it writes, edits and commits nothing, except for edits in `.git/mork-mock/` on a visual card after the baseline step as described under `visual`.
No seat challenges its recap: it is offered for `approve` at once (never self-approved), its `approve-brief` ask given automatically to the project's orchestrator on a delegated non-visual card, while a visual card's waits for the operator's own approve.
Approval keeps the same foreman in its same session, releases the foreman's brief researchers and asks the foreman to build.
The operator's unnamed words in the room reach the foreman at every stage, and `planner` is no recipient on the card.
Visual mode works on a no-planner card at start (`--visual`), by carrying an earlier run's visual mode, through `visual`, and through `mock` reopen, with the foreman owning the baseline step and the mock in the card's room.
The flag is this run's alone: every full start without it gets a planner.
Refused with `--mode report`.
The operator's or an orchestrator's alone: any other seat's start naming it is buzzed.
The window has no control for it.

Cards named after the first are carried by its run (JSON `carries`): `start ex-550 ex-551 ex-552` runs all three in ex-550's one plan, room, clone, branch, review and merge, and decisions stay on ex-550.
When the word after the card is a mode word, the words fill mode, model and effort in order as before (`start ex-550 report opus high`); otherwise each word after the card is one carried card as typed, never split at commas nor read as a JSON list, and mode, model and effort take their flags.
`--carries` takes a list (`ex-551,ex-552`, or a JSON array); an empty word, or a `--carries` naming no card or a blank one, is refused.
Only a full run carries.
Each carried card's board status is set to started with the run's own, the first turn waiting for every one of those writes, after a restart too, and to merged when the run lands, a merged PR included; those writes are held, retried and asked about in the run's room.
A carried card is its run's, so it can't start alone or be carried by another run: while the run is live; while it is closed with a head or publication that hasn't landed (its last bundle still saving included), until a read of the boards finds the run's card closed on its board, a release that a later read missing its file, or a restart, keeps; and while its merged write from the run's landing is still to be done.
A later run going on with work that hasn't landed must name the same carried cards; the room's start button sends them.
Refused, with nothing made, when a carried card is on no configured board or another project, is done or merged on its board, is live, is named twice, is held by another run, or its own earlier run left work that hasn't landed; when a report start names one; when a later run names another set than its unlanded work carried; while the take keeping that closed run's last head is held, which `action.retry` resumes, the run going on from that head once it is kept; and, for the run's own card, while another run holds it.

Example: `mork ctl start ex-550 --mode full --delegated`, or `mork ctl start ex-550 ex-551 --model opus`

### approve

Approves the offered plan: its recap becomes the approved specification, the card moves from brief to build, the card's foreman, the one that challenged it, takes the build in its same session with the approved plan and recorded decisions.
Approval settles in-scope defaults stated in the recap that follow established practice.
A proposed departure from the card or a standing decision requires a separate ask before the recap.
On a visual card it also ends the mock, approved with the plan.
The foreman cuts the deliverables, publishes their tasks and builds them.
Each recap is challenged by the card's foreman before it is offered: with no objection the engine offers that plan; with objections the planner's next recap is challenged again.
No plan is offered or approved while a challenge waits.
A card started with `--no-planner` has no challenge: its foreman's recap is offered at once, and approval keeps that foreman's session for the build.

A delegated plan never approves itself: once offered, its `approve-brief` ask opens and, on a non-visual card, the engine gives it to the card's project's orchestrator automatically without a desktop notice.
Turning `delegate` on while a plan waits gives that open ask without approving the plan.
A visual card's plan, its mock included, is excepted at every level: it waits for the operator's own approve, its `approve-brief` ask showing the sketch, then the mock's link, never given, and an `approve` of it from anyone else is refused.
Every plan waits for this command: only the operator, or the card's own project's orchestrator while its open `approve-brief` ask is given to it, may send it for a plan.
The operator may also give an undelegated plan's ask with `give`, and may still approve any given plan himself.
Approvals stay recorded under their sender, and the approved specification is announced whole in the card's rooms and DMs, naming the operator or `orchestrator:<project>` on his delegation.
Words in the room never approve, a plain yes included.
On a reopened mock (`mock`), `approve` is the operator's alone, his approval of the mock the planner (or the foreman on a no-planner card) offered: its mock asks close, then the foreman is asked to continue the build, naming the decisions recorded since the reopen; only a `built` from a turn that read that request submits.
Approval of a plan agreed after `brief` asks the foreman to continue the build from the new plan, naming the decisions recorded since, bound the same way.
A mock's ask is never given to an orchestrator.
Refused to a sender outside these rules (buzz), when no plan is offered for approval at brief, or on an open mock at build while its offer is down.

Example: `mork ctl approve ex-120`

### mock

Reopens a visual full card's approved mock in the same run, at any stage until its final landing starts: the planner changes it as the operator asks in the card's spec room (or the foreman in the card's room on a no-planner card), and the build waits for its new approve.

The card's whole submission is withdrawn, at build too: a `built` waiting on its bundle or take, its checks, its review round, its verification, its place in the merge queue and a PR's wait all count for nothing.
The review round is superseded, its report never sent, and no fix request is queued; the findings a failed round raised in a report not yet sent, waiting for its proofs' copy or held for the verification's suite, are withdrawn with it, and earlier rounds' findings, already sent, stay open.
A bundle, take or checks run of the withdrawn submission still under way ends counting for nothing: it takes no head, passes or fails nothing, raises no ask and isn't run again, and checks running are stopped.
A push or PR opening of its PR still under way is recorded if it succeeds, the branch's pushed commit or the PR's number kept; if it fails, or a restart finds it never ran, it is dropped with no fix request, ask or retry.
One a restart can't tell the outcome of is held for the operator, who marks it done or drops it: `action.retry` refuses it.
The clone and its commits stay as they are.
A card past build returns to build; one in the merge queue leaves it, saying so in its room.
Running turns finish, and the planner is asked to mock at once, its request in the spec room and the mock ask under it, as at the first mock (on a no-planner card the foreman is asked in the card's room, edits no tracked repository files while open, and offers the mock with `brief-ready`).
On a card with a planner no new turn of the foreman starts until the mock ends, and `built` is refused meanwhile on any visual card.
The mock ends as the first one does: the operator's `approve` of the offer, or visual mode turned off, which asks the foreman to continue the build, naming the decisions recorded since the reopen.
Only a turn that read that request submits: an older turn finishes without submitting and leaves its wake owed.
The strip shows `mock` again, and the host keeps its link.
Answers `{"mock": {"card"}}`.
Refused for a card not in the state, not full or not visual, ended, whose mock is open, whose mock wasn't approved in this run, back at brief after `brief`, and once its final landing has started: a base move started, held or tried, or its PR's merge asked; accepted again once a failed landing leaves the queue.
A local base move still waiting for the card's running turns to end hasn't started: the reopen drops it.
The operator's alone (buzz).

Example: `mork ctl mock ex-120`

### brief

Sends a full card with an approved plan back to brief in the same run, the optional words carried to its planner: its plan is agreed again, then the build continues from the new plan.

The card's whole submission is withdrawn, as `mock` withdraws it: a `built` waiting on its bundle or take, its checks, its review round, its verification, its place in the merge queue and a PR's wait all count for nothing, and a fix request held for its import is dropped.
The review round is superseded; a failed round's findings not yet sent go with it, and earlier rounds' findings stay open.
The clone and its commits stay as they are.
The card returns to brief, its approved plan standing until replaced; one in the merge queue leaves it, saying so in its room.
Running turns finish.
The foreman's old-plan retries and corrections are dropped, including any owed when a running turn ends.
Pending requests and unread news stay available.
`built` is refused until the new plan is approved.
The foreman's open asks stay open, answered or withdrawn as at any time.
The planner is asked in its spec room to agree the new plan, the request carrying the words; on a card with no planner its foreman is asked in the card's room.
Only a recap from a turn that read that request counts.
The foreman challenges it, and it is offered and approved as any plan under the card's delegation level.
Its approval asks the foreman to continue the build from the new plan and the decisions recorded since, and only a `built` from a turn that read that request submits.
The card's review card reads the whole change again against the new plan, its next round's request giving that plan whole for the reviewers kept from before: the rounds before approve nothing more and count for no all-declined pass, and their findings and dispositions stay.
The strip shows `brief` again.
Answers `{"brief": {"card"}}`.
Refused for a card not in the state, not full, ended, without an approved plan or already back at brief, with its mock open, and once its final landing has started, as for `mock`.
The operator's or the card's own foreman's (buzz).

Example: `mork ctl brief ex-120 'The fix belongs in the parser, not the view.'`, or unquoted: every word after the card is the words.

### merge

Says where a card stands in verification or in its project's merge queue.
For a card still at `ready` whose head passed checks, it starts verification.
A landed card answers `landed`.
An unknown card is refused.

Only the operator may, on both kinds of project.
A full card starts verification automatically when its head's checks pass or when none are configured.
The review round and whole suite run side by side, then the card enters its project's merge queue at the back.
Only the front lands, as the `joins-and-the-train` skill says.
On a repository with PRs its project’s forge merge setting applies, as the `pull-requests` skill says.
Answers `{"merge": {"card", "landing", "ahead"}}`: its step (`none`; `queued`, `fetch`, `prepare` and `test` while it is being verified, `review` while its suite ended and its round runs; `passed`, verified and in the queue; `merge` landing; `published`, `approval` and `hand-merged` for its PR) and the cards of the merge queue before it, all of them while it is still being verified.
Refused when it has no head that passed its checks, isn't ready, or (buzz) from anyone but the operator.

Example: `mork ctl merge ex-120`

### stop

Stops a card, or one agent: exactly one of `card` and `agent`.

A card: it and its agents pause, every job of it is stopped, its pending actions are cancelled.
An agent (its key: `ex-120/foreman`, `ex-120/foreman/dm`; a foreman's child is `ex-120/researcher-2`, its place `ex-120/researcher-2/dm` not its key): its running turn is stopped and it pauses.
A card is stopped only by the operator or an orchestrator, any orchestrator on any card; an agent only by the operator or the seat that spawned it (a full card's foreman may also stop or interrupt its planner).
Every other sender is buzzed: no seat stops a card, its own included, and an orchestrator never stops one agent.
Answers `{"stop": {"card"}}` or `{"stop": {"agent"}}`.
Refused when neither or both are given, for an unknown card or one that isn't live, or for an unknown agent.
A card is named with `--card`, an agent with `--agent` and its full key: a bare word fills the card, and a bare `researcher-2` is no agent.

Example: `mork ctl stop --card ex-120`; a foreman stops its child with `mork ctl stop --agent ex-120/researcher-2`

### resume

Resumes a stopped card, or one agent: exactly one of `card` and `agent`.

A stopped or cut turn runs again with a note saying it was cut off before it finished (check whether its outward steps already happened); a failed one's agent is owed a fresh turn instead, as a message would give it: its prompt made from the card as it stands, with the failed turn's news and all since, each read once; a session's first turn run again in that session, as one after a usage limit, and each retry of that one, is asked only to go on with the prompt that began the session when that saved session holds it word for word since any compaction, with a planner's or foreman's standing sections, each unless that prompt sent it as it now stands and no later prompt may have sent it otherwise (all of them through `codex exec`), the reply's reminder and its images' paths, and is sent that whole prompt again otherwise, the reply contract's replacement added when it changed since, its images taken from the latest try holding them; a later turn run again is always sent whole; once any try of a request was cut, each later one keeps that note; cancelled actions run; the card's `error`, `cut`, `checks`, `stuck` and `blocked` asks close, in its spec room too, each live seat whose open `blocked` ask closes is woken for a turn when none is already running or owed, and its failed job runs again: a held run of its verification or its merge queue is retried only this way.
A card stopped while merging steps out while it is stopped, and is verified again.
A card is resumed only by the operator or an orchestrator, any orchestrator on any card; an agent only by the operator or the seat that spawned it (only the operator resumes a planner).
Every other sender is buzzed: no seat resumes a card, its own included, and an orchestrator never resumes one agent.
Answers `{"resume": {"card"}}` or `{"resume": {"agent"}}`.
Refused when neither or both are given, for an unknown card or one that isn't live, or for an unknown agent.

Example: `mork ctl resume --card ex-120`

### fresh

Replaces an agent's session with a new one: its next turn starts afresh.

`agent` is a key (`ex-120/foreman`, `ex-120/foreman/dm`; a foreman's child is `ex-120/researcher-2`, its place `ex-120/researcher-2/dm` not its key), whoever sends it.
In the window, `/fresh foreman` in a room names its card's foreman.
The session ends: nothing resumes it again.
The agent's next turn starts a new session with the start's whole material, the news since its last turn, and one line naming the session it replaces and where its turns are (its place's conversation).
A turn running when it is sent finishes in the old session; the session is replaced as it ends.
It wakes nobody, and works on any seat, mid-card included: whoever sends it carries the new session's reading.
A retry or a correction owed for the old session becomes a fresh turn with its news given back.
The `fresh` command keeps the seat's selection.
A change of CLI at a turn's launch also starts one fresh, its opening line naming the new CLI; a model change on the same CLI keeps the session.
A turn whose CLI can't find its session raises an error ask saying so, offering `fresh` beside `resume`.
Answers `{"fresh": {"agent", "replaces", "pending"}}`, `pending` true while the old session's turn runs.
Refused for an unknown agent, or one with no session yet.

Example: `mork ctl fresh ex-120/foreman`

### pick

Picks an agent's model and effort for its next turn.

`agent` is named as `fresh` names it, whoever sends it.
The operator's own pick, in his window or with his own `mork ctl pick`, offers every configured seat, phase and allowlist model, including both reviewer lists, the roster's and catalog's models including Sonnet 5.5, Opus 5.5, GPT-6.2 Sol, Gemini 4 Neon (`gemini-4-neon`) and Opus 5.5 on agy (`claude-opus-5-5`, label `opus 5.5`), plus Luna (`gpt-6-luna`, label `luna`), Fable 5.2 (`claude-fable-5-2`, label `fable 5.2`) and Astra (`gpt-6.2-astra`, label `astra`).
Its efforts are `low`, `medium`, `high`, `xhigh` and `max`.
Gemini 4 Neon offers `low`, `medium`, `high` and `xhigh`; Opus 5.5 on agy offers `low`, `medium` and `high`.
Only the operator's own `pick` command or window picker can explicitly select Fable, Astra or `max`.
The `spawn` and `start` commands refuse these explicit selections for every sender.
An auto pass can launch its foreman and researchers on these choices through `[[picks.pass]]`.
A permitted `mork.toml` row or launched session naming one still runs, and a configured Fable or Astra row shows its label.
His choices are unrestricted by the foreman allowlists.
The wall constraint applies to every sender, the operator included: an unwalled Claude seat offers Claude models only, because its deny list exists only in Claude Code.
An agy selection requires a walled seat.
A child's configured models appear first, then the rest, each once by CLI and model.
A new model is configured in `mork.toml` first.

A foreman's pick of its card's planner is refused: the planner is its child in name only, its model `start`'s and the operator's.
A baseline seat has no list: only the operator picks it.
A foreman's pick of a researcher or reviewer uses the child selection lists: `[[picks.researcher]]` or only the reviewer list for the review card's kind, `[[picks.reviewer.inside]]` or `[[picks.reviewer.outside]]`.
An absent or empty list permits no foreman-selected model for that role or review kind; the other review kind, seat defaults and the code's catalog never supply permissions.
Other senders picking a child use the same permissions, except the operator.
The entries supply the model and effort permissions, subject to the same wall constraint as spawn.
Other sessions offer the configured and known models at the offered efforts each model takes.
Either model or effort may be named, or both; what is omitted keeps its current or pending value, but the resulting combination must be permitted.
An effort-only pick cannot move a child outside its allowlist.

A refusal names the permission list and its permitted models or, for a refused effort, the permitted efforts for that CLI and model.
For reviewers it names `picks.reviewer.inside` or `picks.reviewer.outside`.

Automatic defaults are separate.
`[seats.<name>]` may set common `cli`, `model` and `effort`, with no role, vendor, model or effort guard.
Nested settings override these fields for the foreman's `work` and `report`, and the review foreman's and reviewer's `inside` and `outside` policies.
Omitted fields inherit common settings, then the phase's fallback.
The retired `spec` and `challenge` keys, `[seats.builder]` and `[[picks.builder]]` are refused as config errors.
A known model with no configured CLI takes its model's CLI.
Inside reviews are linked branch reviews and PRs held by our live cards; outside reviews cover other work.
Each review seat reads its inside or outside default directly from config; those defaults do not restrict the vendor of an explicit reviewer spawn or later pick.
Saved per-card vendors and implementation authorship have no effect.

This example configures the planner on Opus 5.5 and the foreman and every review seat on GPT-6.2 Sol, all at xhigh.

```toml
[seats.planner]
cli = "claude"
model = "claude-opus-5-5"
effort = "xhigh"

[seats.foreman]
work = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }
report = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }

[seats.review-foreman]
inside = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }
outside = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }

[seats.reviewer]
inside = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }
outside = { cli = "codex", model = "gpt-6.2-sol", effort = "xhigh" }
```

The planner defaults to Claude Opus 5.5 at xhigh, under `[seats.planner]`; the retired `[seats.mocker]` is refused as a config error.
A full card's foreman and report foremen default to Codex GPT-6.2 Sol at xhigh.
The report fallback for an unwalled Claude foreman stays on Claude.
Setting `cli = "claude"` with no model also takes that fallback, Opus 5.5 at xhigh, with configured effort over it.
Setting `cli = "agy"` with no model selects Gemini 4 Neon; `cli = "agy"` with `model = "claude-opus-5-5"` configures Opus 5.5 on agy.
When `effort` is omitted on a seat or phase override and the resolved model does not take the seat's default effort (such as `xhigh` on agy's Opus 5.5), it falls back to `high`; an explicit unsupported `effort` holds the seat until an effort is picked.
An agy seat requires a wall and refuses `wall = "none"`.
Common and explicit report settings still override the fallback.
A visual card's baseline seat defaults to Claude Sonnet 5.5 at high under `[seats.baseline]`, always walled: `wall = "none"` there is a config error.
Researchers, and inside and outside review foremen and reviewers, default to Sol at xhigh.
These defaults never constrain the configured models or foreman allowlists.

`model` is taken as `spawn` takes it: a full id or a listed label's first word.
`cli`, `claude`, `codex` or `agy`, goes with `model`: when the same model text occurs on more than one CLI and `--cli` is omitted, a known model takes its own CLI (`claude-opus-5-5` / `opus` stays Claude), and `--cli` is required to pick another CLI (such as `--cli agy` for agy's Opus 5.5) or when the model has no default CLI.
A pick sets that agent's selection independently of its seat defaults.
Its wall, tools, credentials and authority remain its seat's.
A running turn finishes with its launched selection.
A pick waits as pending across restarts and applies on its next turn in the same phase.
Picking what the session already runs records its picker on the launched selection and drops a pending pick.
A change of CLI starts an ordinary fresh session on the next turn: the new session reads the card first, the room gets one line, and the earlier conversation stays.
A same-CLI pick of model or effort resumes the session.

A seat records the CLI, model and effort its phase first launched with and keeps them through config changes until a pick or phase change.
A new review round also refreshes kept foremen and reviewers whose launched selection came from `mork.toml`, taking their current inside or outside setting; a model or effort picked at spawn or later stays, and a pending pick still applies.
Leaving a phase expires its selection, so returning to it takes its latest configured default.
A session recorded before phase tracking takes its default at its next turn.
A pending selection removed from config, incompatible with the seat's wall, or naming Fable, Astra or `max` when it is not the operator's own is ignored, while the session keeps its launched selection.
Automatic defaults need no allowlist permission.

A place's view shows each agent's `setting`, `pending`, `choices` and `efforts`.
Clicking its model and effort opens the window's picker, showing current and next selections and models by vendor, with a note on which changes start a fresh session.
It sends `cli` with every model pick, and the row shows a pending pick as `→ <model> · <effort> next turn`.
Answers `{"pick": {"agent", "cli", "model", "effort", "pending", "fresh"}}`, `fresh` is true for a change of CLI from the one the session runs next: its last turn's.
Refused for an unknown agent, a model not offered, an ambiguous model without `cli`, an effort outside the selected model's permissions, an unknown CLI, or neither model nor effort named.

Example: `mork ctl pick ex-120/foreman --model claude-opus-5-5 --effort high`

### close

Closes a live card's run, with why.

It is stopped, its asks close, its agents go (their sessions end), its head is kept and its clone removed; a PR stays as it is on its forge.
Its board isn't changed: `card.close` does that.
Answers `{"close": {"card"}}`.
Refused when the card isn't live.

Example: `mork ctl close ex-120 superseded by ex-131`

### done

Ends a live report card or review card: its clone goes.
A report card also closes as done on its board, with a closing note (`closed as done: report done`) signed by the sender and its file moved to `completed/`; no board write is made when there is no board file or its status is already the board's done status.
Completed reviews end automatically, PR reviews once their result comments finish posting; no `/done` or PR merge is waited for.
An eligible PR's card reopens on its next head, or when its approved merge finds the destination moved, with its review history, unresolved findings and their original reviewers.
Our card's branch gate ends with our card.

Answers `{"done": {"card"}}`.
Refused for other cards, and for our card's branch review card, which ends with our card.

Example: `mork ctl done pr-web-42`

### preview

Turns a full card's preview worker on, starting or restarting it, or with `stop` turns it off: the operator's call, `/preview` or `/preview stop` in the card's room; while it is on, its foreman may restart it.

A project whose `mork.toml` section names a `preview` command serves a full card's clone once the operator turns its preview on, and only then: the engine runs `sh -c <preview>` in the clone, `MORK_PORT` the card's port, its output in `.git/mork-preview.log` in the clone.
With no `action` it starts the worker, or restarts it when it runs, so it serves the clone's current code; `start` starts a worker that is down, `restart` ends it and starts it again, and both turn it on as well; `stop` ends it and turns it off.
On is kept on the card: the app's start, a reload's included, starts again each worker turned on, until it is stopped or its card closes, lands or is done, which forgets it.
A worker that exits on its own stays down until started again.
Report and review cards serve none.
Each call that turns it on, and each restart by its foreman, says so in the card's room and its DMs, with its link, as a Mork announcement.
Answers `{"preview": {"card", "on", "running", "url", "log"}}`, `url` being `http://<card>.localhost:<port>/`, the card's own host name, so its login cookie is its own.
Refused for a card whose project names no `preview`, one that isn't live, isn't full or is still starting, an action that isn't one of the three, or a worker that couldn't start, with why.
The operator's, but for the card's own foreman's restart (no `action`, or `restart`) while its preview is on, refused while it is off; `start` and `stop` are buzzed to the foreman, and every call to anyone else.

Example: `mork ctl preview ex-550`, `mork ctl preview ex-550 stop`

### demo

The operator's `/demo` in the card's room turns a full card's preview on and asks its foreman for the changed page's direct link and a brief reminder of the change.

It does what `preview` with no action does, then wakes the foreman to raise its hand with the link and reminder.
The foreman creates data only when the change needs it and no existing data shows it, following the project's preview skill when one is among its skills.
Answers `{"demo": {"card", "on", "running", "url", "log"}}`.
Refused as `preview` is, and for a card at brief, with nothing built yet.
The operator's alone (buzz).

Example: `mork ctl demo ex-550`

### prs

Open PRs for a project with `forge.kind` set to `bitbucket` or `github`, as the poller last read them.

The poller reads each hosted project's open PRs each `poll_seconds` (a minute by default), so a PR opened since its last read may be missing.
The list includes GitHub fork PRs, which have no review card.
The `pull-requests` skill explains which PRs get a review card.
Answers `{"prs": {"project", "prs": [{"pr", "title", "author", "source", "destination", "updated", "link"}]}}`: `source` and `destination` are its branches, `updated` the time of its last update, `link` its forge page.
An empty list means none is open.
Refused for an unknown project, a project with a local forge, or one the poller hasn't read since Mork started.
A read.

Example: `mork ctl prs web`

### review

Opens a review card for an open PR on a hosted project, a colleague's or our own when no review of it is live: `pr-<project>-<n>`.

Its diff is against the PR's destination branch, its clone at the integrated candidate.
It is verified off the merge queue, its round and its suite side by side; on a colleague's PR its findings, or “No findings.”, and a failed suite's comment are posted once both ended, while our own PR gets only “No findings.” when clean and its findings and failed suite go to its foreman alone, as the `pull-requests` and `review-cards` skills say.
Nothing merges from a review alone.
Answers `{"review": {"card"}}`.
Refused for a project with a local forge, a PR not open as last read, a GitHub fork PR, or a review card already live.

Example: `mork ctl review web 42`

### review.merge

Only the operator can run `review.merge`, which opens a review card for an eligible PR as `review` does.

On projects with approval merging, this allows the operator's approval to merge the verified head into its destination after the review closes.
Manual projects wait for the operator to merge on the forge, as the `pull-requests` skill says.
Even a clean review leaves the merge to the operator.
A merge refused is held for the operator.
Answers `{"review.merge": {"card"}}`.
Refused as `review` is, including for GitHub fork PRs.
An agent or orchestrator sending the command gets a buzz.

Example: `mork ctl review.merge web 42`

### review.branch

Opens a review card for a branch of a project's origin that has no PR: `branch-<project>-<branch>`, the branch's name made an id's.

Its diff is against the project's base, its clone at the branch's tip as the card's fetch reads it.
It is verified as a PR's review card is.
Its round starts from changes since its last reviewed source head and traces fixes and merged base changes into affected callers and dependents.
Its verdict and findings are reported to the operator once its round and its suite both ended; with no PR, nothing is posted.
Completed reviews close automatically, retaining unresolved findings and their reviewers for a later request.
It only reports: nothing lands the branch.
Asked again while the card is live, after a fix, the same card goes through the steps again at the branch's tip, its foreman, its reviewers and their open findings kept; an ended card's next run carries its review.
Answers `{"review.branch": {"card"}}`.
Refused for an unknown project, the base branch itself, a card stopped (`resume` it first) or landing, a branch our live card works on, or one whose review card of ours is under way; a card started over an ended one of ours carries none of its review; a branch origin doesn't have is said on the card.

Example: `mork ctl review.branch web feature/reader`

### post

Posts a PR review card's findings as inline comments on its PR.

One comment per finding; one already made isn't made again.
Completed PR reviews post automatically; `post` can request any findings not yet posted.
Automatic closure waits for result posting to finish.
Answers `{"post": {"card", "comments"}}`, the new ones counted.
Refused for a card that isn't a PR's review card (a branch's has no PR).

Example: `mork ctl post pr-web-42`

### findings

A review card's findings, named by the review card or by the card it reviews: every finding its rounds raised, oldest first.
Each has its number `n`, as `resolve` takes it, its `round`, who raised it (`by`), its `file`, `line`, `summary` and `proof`, and its `state`: `dropped`, `declined`, `resolved` or `open`, the first that holds in that order.
Answers `{"findings": {"card", "of", "findings": [{"n", "round", "by", "file", "line", "summary", "proof", "state"}]}}`, `card` the review card and `of` the card it reviews, `null` for a PR's or a branch's own.
Anyone who may read `card.show` may read it.
Refused for an unknown card or one no review card reviews.
A read.

Example: `mork ctl findings ex-550`, or `mork ctl findings pr-web-42`

### triage

Answers an open findings `triage` ask for `card`, assigning every finding of the failed review round to `--fix`, `--decline` or `--foreman`.
A `triage` ask opens when delegation is off, or as a red stalemate escalation on a delegated or self-sustained card when two consecutive completed non-superseded rounds under the current plan both fail with findings, none of the prior round's unresolved findings were resolved and every one repeats unresolved; otherwise on delegated and self-sustained cards, failed-round findings go straight to the foreman as Foreman decides.

Each list holds finding numbers `#n` on the card's linked review card (comma-separated or a JSON array of strings).
Every finding of the triage ask must appear in exactly one list; a missing, doubled, unknown or non-numeric entry is refused with nothing applied, as is a paused card or a card with no open triage.
A finding already resolved by hand since the ask opened keeps its state and its choice is ignored.

The engine writes `declined` on each finding chosen `--decline` (as `resolve --declined`, journaling `review.resolved` and posting the decline reply on any PR comment thread), closes the triage ask, posts one summary line in the card's room, and journals `review.triaged`.
When every finding is declined and the candidate's suite passed, verification starts again on the same head without waking the foreman (skipping checks that still stand).
Otherwise, one request is sent to the card's foreman carrying only the `--fix` and `--foreman` findings (marked `fix` and `yours to decide` with their original round positions and proofs), naming declined findings as closed, and carrying any suite failure from the same candidate.

The operator's; the card's project's orchestrator's only while the card's open `triage` ask is given to it.
Anyone else is buzzed.
Answers `{"triage": {"card", "round", "fix", "decline", "foreman"}}`.

Example: `mork ctl triage ex-550 --fix 4,5 --decline 6 --foreman 7`

### resolve

Marks finding `n` of a review card verified fixed, or with `--declined` ruled out by a decision on the card (the operator's or one recorded on the orchestrator's delegated answer), and resolves its comment's thread on its PR.

`n` is the finding's number as the card's round requests list it (`#3`), each with whoever raised it.
The reviewer that raised a finding verifies its fix and resolves it; the review card's foreman resolves its own alone; the operator or an orchestrator may resolve any, at any time; the foreman of the card a linked review card reviews may resolve any of its findings with `--declined` only, at any time, when the finding reached it marked Foreman decides (from triage or directly on a delegated card) or on the operator's word or the project's orchestrator's delegated answer.
Anyone else is buzzed.
An agent's resolve counts in a turn of the round under way, one begun after the round was asked, on its commit: between rounds, or from a turn begun before the round was asked, it is refused, and the finding stays open; the operator's, an orchestrator's, and the reviewed card's foreman's `--declined` on a linked review card count at any time.
A finding posted on its PR has its comment's thread resolved by the review bot, at once or as soon as its comment is posted; a finding resolved `--declined` first gets the review bot's reply "This finding was declined.", and `--dropped` posts its reviewer’s own words before resolving the thread if needed.
Once a round passed, the card's reviewers with no finding left to verify are released; one with a finding open stays for it.
Answers `{"resolve": {"card", "n", "comment"}}`, `comment` whether a thread is resolved now; one already resolved answers `already`.
A finding raised again once resolved is a finding of its own, under a new number, its comment and thread new.
Each round's request lists the findings the operator resolved, those declined on a linked review card and those reviewers dropped, with their summaries, and tells its foreman not to raise them again.
A finding open asks a round of each run of its card, though nothing it has is left to review, as when its change was reverted: its raiser verifies it there, and the run isn't clean until it is verified.
A failed round of our card's branch card whose findings, each named by its verdict, are all declined counts as passed for the submitted head, base and contents it read: that head's checks lead to no new round, and its candidate is verified once its suite passes.
Refused for a card that isn't a review card, or a number it has no finding under.

Example: `mork ctl resolve pr-web-42 3`, or `mork ctl resolve pr-web-42 4 --declined`

`--dropped '<words>'` drops a finding because a colleague’s reason on its thread holds.
It requires a person’s reply read on that thread and keeps the same sender and round-turn guards.
The reviewer gives its own one or two sentences.
Its reply posts once, then the thread is resolved if it was not already.
A person resolving the thread alone closes no finding.
`--dropped` and `--declined` cannot be combined.
Words refused by the engine's outward check close no finding: reword the same reply.
Answers `{"resolve": {"card", "n", "action"}}` for the drop’s reply action.
Later rounds list the drop as settled, not to be raised again.

Example: `mork ctl resolve pr-web-42 3 --dropped 'The address has no account, so the history entry is not needed.'`

### answer

Posts the finding’s reviewer’s explanation of why a colleague’s reason on its thread does not hold, once per finding, leaving the finding open for the operator’s ruling.

`mork ctl answer <card> <n> '<words>'` keeps `resolve`’s sender and round-turn guards and requires a person’s reply read on the finding’s thread.
Its words are the reviewer’s own one or two sentences.
Words refused by the engine's outward check consume no answer: reword the same reply.
A second answer is refused; later replies are still read at each round and may justify a drop with `resolve --dropped`.
Resolving a thread alone closes nothing.
At the head where the reviewer answers, no additional “Still open at” is posted; routine reminders are also suppressed on threads read as person-resolved or whose resolution could not be read.
The last successful thread read is retained when a later read fails.
Answers `{"answer": {"card", "n", "action"}}` for the reply’s action.
Refused for a resolved finding or one with no PR comment or head.

Example: `mork ctl answer pr-web-42 3 'This also affects recipients with accounts, whose delivery history is still required.'`

### rereview

Reviews our card's PR again on its current head, nothing pushed, at a head no verification passed where its PR's review card reviews it: its last round failed, and a decision on the card (the operator's or one recorded on the orchestrator's delegated answer) rules out what is left of its findings, or its last run failed, a flaky test outside the card's files for one.

The PR's review card (`pr-<project>-<n>`) is verified again on the head it has, or, ended, reopens on the PR's head as the poller last read it, with its rounds, findings, threads and reviewers.
The failed round's files aren't approved, so its verification asks an ordinary round of them, which lists the card's standing decisions and its open findings, each for its raiser to verify, or to resolve `--declined` where a decision rules it out.
Where its rounds already approve the head, no round is asked: only the suite runs again, on the same head merged with the current master.
A pass with no finding open makes the run clean, and the operator's approve ask comes as for any head.
The engine never runs it by itself; a `built` of the same head runs nothing.
Answers `{"rereview": {"card", "review"}}`, `review` the PR's review card.
Refused for a card not in the state, a review card, a card with no PR or no review card of it, a review card stopped, one whose round or run is under way, one whose last round and last run didn't fail, and an ended one whose PR isn't open as the poller last read it.
The card's own foreman's on its own judgement, the operator's or an orchestrator's (buzz): anyone else's is buzzed.

Example: `mork ctl rereview ex-260`

### deliverables

Cuts a card into its deliverables: named parts of its work in order, each with a task list of its own, posted in its room.

`names` takes a string with one deliverable's name per line, in order, or a JSON array of strings (directly through `--json`, or encoded in a string); on the command line each further argument is a name of its own (`deliverables ex-550 'The invoice form' 'The totals modal'`), as if joined by newlines; a leading list marker (`-`, `*`, `1.`) is dropped and a blank line or empty array item skipped.
A name is its deliverable's only label, never a number.
Sent again, it re-cuts the card, with no approval: a deliverable whose name is in the new list keeps its tasks and its current task, and the current deliverable stays current while its name survives, else the first is; a deliverable left out is dropped with its tasks.
A new one has no tasks until `plan` gives it some.
The cut is posted in the card's room as a list, an update that wakes nobody.
At build the foreman cuts deliverables before publishing their tasks.
Answers `{"deliverables": {"card", "deliverables", "seq"}}`, `deliverables` counted, `seq` the message's.
Refused for a card that isn't live, one with no foreman, or a list with no name, two of one name, more than 30 names, or a name over 200 characters.

Example: `mork ctl deliverables ex-550 $'The invoice form\nThe section header\nThe totals modal'`

### plan

Publishes the tasks of one of a card's deliverables, in order, shown with the card and posted to its foreman.

`deliverable` is its name, exactly as the card's deliverables give it, or its number, from 1, as `card.show` lists them (digits alone are a number); on a card cut into none, `1` is the whole card, which the plan makes its one deliverable.
`tasks` takes a string with one task per line, in order, or a JSON array of strings (directly through `--json`, or encoded in a string); a leading list marker (`-`, `*`, `1.`) is dropped and a blank line or empty array item skipped.
The tasks replace that deliverable's earlier ones whole, its first task current, and it becomes the current deliverable; the other deliverables keep theirs.
The window shows the open card's deliverables in the Plan section of its side column, under its Notes, as sections that fold, the current one unfolded.
The plan is posted in the card's room to its foreman, numbered and naming its deliverable.
An agent's plan is an update that wakes nobody.
The foreman reads other agents' task lists in its next turn's news.
The operator's or an orchestrator's plan is a request that wakes the foreman.
The foreman publishes each deliverable's tasks.
Earlier task lists stay in the conversation.
Answers `{"plan": {"card", "deliverable", "tasks", "seq"}}`, `tasks` counted, `seq` the message's.
Refused for a card that isn't live, a deliverable it doesn't have by that name or number, or a plan with no task, more than 100 tasks, or a task over 400 characters.

Example: `mork ctl plan ex-550 2 $'Read the reader\nWrite the failing test\nSkip the repeated header'`

### plan.task

Marks the task of one of a card's deliverables being worked on now.

`deliverable` is named as `plan` names it, and `task` is the task's number in its plan, from 1, or its text exactly (digits alone are a number); a text that two of its tasks share is refused, naming their numbers: the tasks before it show as done, those after it to come, and the deliverable becomes the current one.
It posts nothing and wakes nobody.
Answers `{"plan.task": {"card", "deliverable", "task", "of"}}`.
Refused for a card that isn't live, one with no plan, a deliverable it doesn't have or with no tasks, or a number outside its tasks, or a text it has none or several of.

Example: `mork ctl plan.task ex-550 2 3`

### card.show

A card whole: its board file's text, notes and acceptance, its run, its open asks, its brief, its deliverables and its live seats.
Its notes are told apart by Mork's recorded writes to its file in whichever board folder it then was (`tasks/`, `completed/`, `archive/tasks/`), notes opening with the same line paired with those writes in order, the newest on top, so a closed card's older multi-line note stays whole.

Answers `{"card.show": {"card", "project", "title", "status", "description", "notes": [{"number", "text"}], "acceptance": [{"text", "done"}], "run", "carries": [{"card", "title"}], "asks", "brief", "deliverables": [{"n", "name", "tasks", "task", "current"}], "seats": [{"name", "role", "place"}], "delegated", "self_sustained", "visual"}}`; `carries` lists the cards its run carries, titled as their boards hold them, empty when it carries none; `brief` says whether its draft waits for its foreman's challenge until it is offered, then for approval, or is approved, or an older build started it with nothing to clarify; `deliverables` are its run's in order, each with `n`, the number `plan` and `plan.task` take it by, its `tasks`, `task` the current one's number (`null` while it has none), `current` true for the one worked on now; none before any plan; `seats` lists the card's recorded agents in the window's session order: its spec room's, its build room's, its DMs', then its children, each with its full agent `name`, `role` and message `place`, leaving out seats in their closing turn, an empty list when none are live; `delegated` and `self_sustained` its delegation level as `delegate` describes, and `visual` its visual switch.
A read.

Example: `mork ctl card.show ex-120`

### card.create

Writes a new card on a project's board, in Backlog.

`id` is the next free one unless given (`<letters>-<n>`).
The same request sent again gets the first one's answer.
Answers `{"card.create": {"project", "action"}}`, with `id` and `file` once written.
Refused for an unknown project, a title of more than one line, an id taken or malformed, or (`leak`) a secret in its text.

Example: `mork ctl card.create web "Fix login redirect" Users land on / after login`

### card.change

Changes a board card: any of its fields, and a dated note on top of its description.

`--note` adds a dated note signed with the sender; `--decision` does the same with its text prefixed `Decision: `.
Give one or the other, never both.
`--withdraw <n>` (`3` or `#3`) withdraws the marked decision that is note n: every note has a number, the oldest's #1, which a newer note never changes, shown in `card.show`'s notes (`{"number", "text"}`) and before each note in the window.
It adds a dated note `Withdrawn decision #<n> - <its words>` and the decision itself stays, so the history is kept; the window shows the decision struck through, and it leaves the specification the blind view and the review card get.
Give it alone; it is refused when note n doesn't exist, isn't a marked decision or was already withdrawn.
A note from before in either older form still withdraws the decision it took back when it was written, its card's notes told apart as they were then: `Withdrawn decision: <words>` the newest standing one under it with exactly those words, `Withdrawn decision #<n>: <words>` the one its number named.
Where a closed card's older notes were then cut at their first line, either is shown as `Withdrawn decision #<n>: <words>`, numbering that decision as its notes are numbered now.
The engine can't tell who asked for a withdrawal, so say in your reply which instruction of the operator's it follows.
The window shows every note.
Marked decisions not withdrawn alone reach the blind view, newest first, under "The operator's later decisions", part of the specification.
Old notes stay unmarked.
Otherwise one write: `description` replaces the text under the notes, `labels` the whole list, `acceptance` adds criteria (one per line); `title`, `status`, `priority` and `parent` are set.
Agents of a live card get its new text (title, description under the notes, acceptance) by a Mork announcement: an edit is context, never an approval.
A new note reaches the card's own places as an update; a marked decision, or the withdrawal of one, also reaches its live branch or PR review card's room and agents.
Other notes stay out of the review.
Give each field with `--<field>`: extra plain words fill the optional text fields in order.
Answers `{"card.change": {"card", "action"}}`, with the write's `file`, `before` and `after` (the file's hashes) once written; a write the board refuses is refused with its words.
Refused when no board holds the card, nothing is given, a value is malformed, the description can't be replaced safely (a note may sit in what it would replace: add a `--note` or `--decision` instead), or (`leak`) a secret would be published.

Example: `mork ctl card.change ex-120 --note "waiting on the payments API"`

### card.close

Closes a card on its board, as `done` or `dropped`, with a note.

`done` sets its done status and moves its file to the board's completed folder; `dropped` archives it.
Mork's run of the card isn't touched: `close` or `done` does that.
Answers `{"card.close": {"card", "action"}}`, with the write's `file`, `before` and `after` (the file's hashes) once written; a write the board refuses is refused with its words.
Refused for another word than done or dropped, or a card on no board.

Example: `mork ctl card.close ex-120 done shipped in 2.3`

### actions

The outward actions not finished: pending, started or held, with how many were dropped.

Answers `{"actions": {"actions": [{"id", "kind", "status", "target", "card", "words", "ask", "attempts", "cancelled"}], "dropped": n}}`.
A read.

Example: `mork ctl actions`

### action.retry

Runs a held action again.

For a backup push, each attempt sends the current local base tip, including commits landed since the failed attempt.

Its asks close and it is pending again.
A door call is never run again (its requester asks again).
A merge's retry is a merge, and needs the operator's consent; a PR's checks again that his approval stands on its head.
A held PR opening takes its wording as it is now, checked again: on a project with `tickets.kind` set to `azure-devops`, from the card's newest `PR wording` note (first line `PR wording`, then `Title: <summary>`, then `<ticket number>: <note>` lines; see the pull-requests skill).
A card with tickets on such a project whose note lacks a summary or a ticket's note stays held, with what to write; no fallback is published.
Any other card's title is the board title in a few words.
A board note held with every attempt refused by its content check before anything was replaced is made again from the card's file as it is now, and written only over that content; any other board write keeps the content it was made from, and refuses again if the file changed, as does a note one of whose attempts may have written.
Answers `{"action.retry": {"action"}}`.
Refused when it isn't held, or for a push or PR opening of a submission a reopened mock withdrew.

Example: `mork ctl action.retry act-12`

### action.drop

Gives up a held action.
A door call's is the operator's alone; its requester is told.

Answers `{"action.drop": {"action"}}`.
Refused when it isn't held.

Example: `mork ctl action.drop act-12`

### action.done

Marks a held action done by hand, with a note: what follows it runs as if it had returned.

A door call's is the operator's alone, and only one that ran.
Answers `{"action.done": {"action"}}`.
Refused when it isn't held.

Example: `mork ctl action.done act-12 merged it by hand`

### test.lease

Asks for a test slot: what `mork test` sends.
Use `mork test` instead.
A researcher's or baseline seat's run takes a project slot and a machine test run, but not its card's slot.

`kind` is `checks`, `run`, `file`, `walls` or `suite`; `command` the paths given, which `checks` ignores, the whole command, or a `walls` file and optional filter, preceded by `--no-default-features` for production tests.
A project with no checks needs no slot, and with no slot free the answer waits: the connection is first sent `{"waiting": {"id", "why"}}`, with no `id` of its own at the top, before the answer.
Answers `{"lease": {"id", "argv"}}`; a `checks` lease's `argv` runs the project's checks command, with `{fork}` for the branch point, and its answer adds `base`, the project's base branch, `fork`, the card's branch point for a clone with no `origin/<base>`, `checks`, its `command`, `folder` and the folder's commit and state as Mork reads it (`at`: `commit`, `dirty`), or null.
A `walls` request instead waits for its engine job to end and answers `{"walls": {"exit", "stdout", "stderr"}}`; its seat and words are checked as `mork test --walls` describes, before any slot.
The slot is held while the connection stays open; a `walls` slot remains held until its job has stopped after disconnection.
Refused with no clone to test in, a `file` on a project with no `test_file` or with no paths, and for a `suite`, with arguments or none: the suite's command is verification's and the merge queue's.

Example: `mork ctl --json '{"test.lease": {"kind": "run", "command": ["true"]}}'`

### test.resume

Takes a lease back after a reconnect: `mork test` does it.

After a start or a reload, a lease not taken back within 30 s is freed.
Answers `{"lease": {"id"}}`.
Refused when the slot was lost, or isn't yours.
A `walls` run cannot resume: it stops and must be requested again.

Example: `mork ctl test.resume lease-1760600000000`

### test.done

Reports a run's exit and frees its slot: `mork test` does it.

No run is remembered as a pass: only verification's and the merge queue's are.
Answers `{"test.done": {"lease"}}`.
Refused as `test.resume` is, and for a `walls` run: the engine frees its slot once its job has ended.

Example: `mork ctl test.done lease-1760600000000 0`

### door

Runs an operation on a machine outside your wall, through the engine.
The engine reaches it over the operator's own access (his ssh alias, his account on the service) with the box's own tools, nothing of Mork's installed there, and reads the credentials itself: none is ever shown to you.

`op` is one of `sql.select {query}`, `mongo.find {collection, filter?, projection?, sort?, limit?}`, `http.get {endpoint, params?}`, `file.tail {log, lines?}`, `file.cat {root, path}`, `file.get {root, path}`, `sh.run {command, args?}`, `misc.run {argv}`; `mork ctl door --help` lists them, `mork ctl door --target <t> --help` describes a target.
A request is resolved against the target's allowlist; an allowed call runs when its target is free.
A `misc.run` becomes the operator's ask, answered with `door.once` or `door.no`.
It opens in the build room on a two-room card and in your place otherwise.
A held call's ask opens in the same place.
News of an asked or held call's outcome reaches you in your place and wakes you; direct calls never post news when their caller disconnects, and a same-turn retry still gets a finished call's answer.
Any other call outside the allowlist is `not allowed`.
The same request in the same turn is the same call.
A read (every `op` but `sh.run` and `misc.run`) whose outcome turns out unclear, cut at its deadline, by an ssh drop or by a restart that finds it sent with no receipt, is not held: it answers `failed`, its `text` starting `cut, ask again`, and asking again runs it as a new call; an unclear `sh.run` or `misc.run` is `held` for the operator, never run again blind.
A `sql.select` whose server didn't take the kill of its query fails the same way, its `text` saying the query may still be running on its target, and the operator gets a note in main; until it has ended there, the same query from the same requester is `not allowed`, its `why` saying when it clears.
The answer is ok even when not allowed: `{"door": {"status", ...}}`, `status` one of `done`, `failed`, `ask raised`, `held`, `busy`, `not allowed`, `unsupported`, with the output (`rows`, `text` or `json`, `truncated`, `elapsed`, a database's `server_us`, a cut SQL answer's optional `server_us_why`, and a copied file's `file` and `size`).
Parameters of the wrong shape are `invalid`.
Invalid requests, unsupported operations and writes remain refusals.

Calls run one at a time per target, in ask order.
A queued request gets a waiting line on its connection, printed on stderr by `mork ctl`, and its window step says it waits.
The line names the running call's action id, requester, op, elapsed milliseconds and time limit, and how many calls wait ahead; never another requester's query or parameters.
Each request waits at most 60 seconds before its call starts, then answers `busy`: its `why` carries those same facts, says nothing ran and says to ask again.
Requests sharing a call each get their own waiting line and 60 seconds; the call is dropped unstarted when its last waiter times out.
A queued read (every op but `sh.run` and `misc.run`) is also dropped at once when its last caller disconnects (a restart included), with `stopped: its caller went away before it ran`.
A started call keeps running, except `sql.select`, which stops when its last caller disconnects.
A call dropped as busy or because its caller went answers no later request: asking again, in the same turn too, makes a new call.
A queued non-read call nobody waits on, including an approved `misc.run` or a `sh.run` pending across a restart, has no queue time limit and runs when its target is free.

Each operation keeps to its bounds: `sql.select` runs one read-only statement on any table the operator's login on the target may read, `information_schema` and the `sys` schema refused (appending `[targets.<id>.sql] schema_hint` when configured), a replica (`read_only = true` in its `[sql]`) with its own row cap and timeout; `mongo.find` only finds, its code-executing operators refused; `http.get` runs any GET path at the configured API origin; `file.get` copies one regular file under a named root into the asking seat's `io/` folder (`MORK_IO`), at most 20 MB.
For `http.get`, `[targets.<id>.http]` sets the HTTPS `origin` and the OAuth2 `client` credential reference.
The door obtains a token from `token_path` (default `/oauth2/token`) at that origin and uses it for the GET.
It sends `scope` only when configured.
Set `scope = "default"` if the service requires that literal value.
An orchestrator's `file.get` outside a turn copies to `<mork home>/door/files/<call id>-<file name>` (`<call id>` is the action id), private to the operator's user and outside every wall.
It is kept 14 days; the daily prune removes older copies.
The answer's `file` gives the full path; the caller never chooses a destination.
`elapsed` is the dispatcher's milliseconds for the run, its ssh and the client's login included, not the engine's bookkeeping; `server_us` is the server's own time for the statement in microseconds, null when it can't be told, including a SQL client cut at the target's size cap: unknown, never cheap.
In that case, `server_us_why` says the answer was cut at that cap before the query's time could be read from its server; a completed read trimmed afterwards keeps its time.
The `the-door-and-databases` skill says what each reads and refuses and how to judge a query's timing; `door.help` says what a target allows.

Example:

```sh
mork ctl door --target staging --op sql.select --params "$(cat <<'JSON'
{"query": "SELECT id FROM app.users WHERE name = 'O''Neil'"}
JSON
)"
```

### door.once

The operator runs a door call waiting on his ask, once.

It runs only if its target and allowlist still resolve as they did when checked.
Answers `{"door.once": {"ask", "action"}}`, before it runs.
The operator's alone (buzz).

Example: `mork ctl door.once web-1/door/1`

### door.no

The operator refuses a door call waiting on his ask: its requester is told.

Answers `{"door.no": {"ask", "action"}}`.
The operator's alone (buzz).

Example: `mork ctl door.no web-1/door/1`

### door.help

The door's targets, as the engine describes them: their services, tables, logs, optional `schema_hint` and limits, never an alias, host, login or credential.

Answers `{"door.help": <target>}`, or every target.
Refused when `targets.toml` can't be read, or the target is unknown.
A read.

Example: `mork ctl door.help staging`

### give

Gives an open ask to its project's orchestrator to answer for the operator, on any ask but a mock's or a visual card's plan's, which are the operator's alone, a specification's included, including a design question marked by a planner or foreman.

The stream of the ask's card's project gets a `given` line for it, as the ask's `ask-opened` line has it: at once while that orchestrator is hooked, else with its next hook's catch-up.
The ask shows "sent to orchestrator" until it closes; the operator may still answer it himself, and the click answers and approves nothing.
While an open plan ask (`approve-brief`) is given, that project's orchestrator may approve it with `approve`; the operator may still approve it himself.
Answers `{"give": {"ask"}}`.
Refused for an ask that isn't open, already given, a visual card's plan ask, a mock step's (which wait for the operator's own `approve`), or an ask that belongs to no project when `[mork] project` is unset.
The operator's alone (buzz).

Example: `mork ctl give ex-550/answer/1`

### opinion

Asks the ask's project's orchestrator for its opinion on an open ask, leaving the operator to decide.

The stream of the ask's card's project gets one `opinion` line: `ask` carries the ask's details as on `ask-opened`, and `text` tells the orchestrator to give the operator its opinion in its own chat and answer nothing to the seat.
It arrives live while hooked, else at its next hook; on its first hook ever it arrives only if the ask is still open and not given.
Nothing is posted in any place or saved on the ask: it stays open, the operator's, unchanged.
It may be sent again.
Answers `{"opinion": {"ask"}}`, journals `opinion`.
Refused for an ask that isn't open, is already given, or belongs to no project when `[mork] project` is unset.
The operator's alone (buzz): the orchestrators and every agent are refused.

Example: `mork ctl opinion ex-550/answer/1`

### delegate

Sets a card's delegation level: `delegate <card>` selects delegated, `--self-sustained` selects the second level, and `--off` ends delegation.
This command is the operator's alone; an orchestrator names either enabled level only as it starts a card, with `start --delegated` or `start --self-sustained`.
The window has one three-position control, off, delegated, self-sustained.

When the level changes on a live card, the engine posts an update to the full card's planner in its spec room (its foreman in the card's room on a card with no planner) or the report or review card's foreman.
The seat reads it in its next turn's news.
An unchanged level or an ended card posts no update.

At brief, the planner or no-planner foreman puts convention-following defaults in the recap.
A specification question calls for two defensible options with a real tradeoff or a proposed departure from the card or a standing decision.
On a delegated or self-sustained card, seats never decide specification questions alone, never raise an ask to weigh failed review findings, and never send specification questions as direct messages to the orchestrator: every open question and held challenge objection opens as a seat `answer` ask in the seat's room, given to the card's project's orchestrator automatically without a desktop notice (including design-tagged asks), while failed review rounds with findings skip triage and go straight to the card's foreman as Foreman decides (waiting for any candidate suite run so both results travel in one request), except that a consecutive-round stalemate where none of the prior round's unresolved findings were resolved and all repeat opens a red `triage` ask for the operator instead of looping.
At the first level (`delegated`), the seat names in the ask's text when an answer would depart from the card's words, reverse an earlier decision of the operator's or need his own words, and the orchestrator leaves that ask open for the operator and brings it to him.
At the second level (`self-sustained`), the orchestrator answers all given asks, departures and reversals included; the wake line carries the card's level (`(delegated)` or `(self-sustained)`).
Our card's branch review card starts at its card's level.

A delegated plan never approves itself: once offered (after the foreman's challenge, or at once on a `--no-planner` card), its `approve-brief` ask opens and, on a non-visual card, the engine gives it to the card's project's orchestrator automatically without a desktop notice, so the orchestrator or the operator approves it with `approve`.
When the card's project's orchestrator writes to the specification holder in its room while that plan ask is given (and is not answering the holder's unanswered mechanical request), the waiting recap is withdrawn, the ask closes and the message is marked as on the operator's delegation.
Turning delegation on from off gives the card's open ungiven `answer` asks and waiting non-visual `approve-brief` ask (journaling `give`) without approving the plan, and closes any open ungiven non-stalemate `triage` ask, sending its findings straight to the card's foreman as Foreman decides.
Switching from `delegated` to `self-sustained` re-emits `given` stream lines for the card's open already-given `answer` and non-visual `approve-brief` asks so the orchestrator wakes under the new level for asks it left to the operator.
Asks already given stay given until they close, even after delegation is turned off; a stalemate `triage` ask stays open on the operator across `delegate` toggles, and an already-given `triage` ask stays with the orchestrator.

A `blocked` ask, a visual card's plan, a mock, a merge, a PR approval, a door call and every engine ask are never auto-given and keep their desktop notices.
Direct request messages to the orchestrator are for mechanical needs only.
For each newly opened actionable engine ask on a delegated card, error, cut, stuck, checks, held or alarm, the engine posts a waking request to its project's orchestrator in the card's room with the ask's words and answer commands.
The ask stays open, the operator's, never marked sent to orchestrator; a command that answers it closes it.
Held merge or door actions get no request.
The explicit `give` and `opinion` buttons remain as they are.

Kept on the card across restarts and later runs unless `start` names a level.
`card.show` reports `delegated` and `self_sustained`: both false is off, only `delegated` true is the first level, both true is the second.
Answers `{"delegate": {"card", "delegated", "self_sustained"}}` and journals `delegate` with those fields.
Refused for a card not in the state, turning on a card that has ended, or both `--off` and `--self-sustained`.
The operator's alone (buzz).

Example: `mork ctl delegate ex-550`, `mork ctl delegate ex-550 --self-sustained`, `mork ctl delegate ex-550 --off`

### visual

Turns a live full card's visual mode on, or with `--off` off.

When visual mode changes on a live full card, the engine posts an update to its specification holder while starting or at brief (the planner in the spec room, or the foreman in the card's room on a no-planner card) and to its foreman in the card's room after approval.
The seat reads it in its next turn's news.
An unchanged value posts no update.

Visual mode makes the mock part of a full card's specification: a baseline of today's pages at brief, changed by the planner (or the foreman on a no-planner card) beside the plan and approved with it.
The foreman, seated at start, has the baseline built and leaves tracked repository files untouched before the plan's approve.
Once its checks pass, or when none are configured, a visual card goes straight to `ready` and on to verification, then the merge queue or publication on a repository with PRs, like any other full card.
Turned on after the plan's approve, no mock opens and the card stays where it is; turned off after it, nothing moves but a reopened mock, which ends as its approve would.
`/preview` and `/demo` remain the operator's commands; nothing waits on them.

Once the clone is made, a visual card's mock opens at its baseline step, its host's port taken: the engine posts the foreman's "Build the baseline" and, on a card with a planner, the planner's "Agree the specification", whose turns wait until that step ends; on a no-planner card, "Agree the specification" comes once the baseline ends or visual mode turns off.
The foreman reads the card and the code, names the pages the card changes, and spawns its baseline seat (`spawn <card> baseline '<task>'`, on its configured default) to reproduce each as it stands today in the clone's `.git/mork-mock/` (untracked), keeping any mock already there, an earlier run's included, and filling only what it lacks.
Its `built` from a turn that read that request ends the step and submits nothing: the mock is then the specification holder's, the baseline seat released; with no page to reproduce it spawns nothing and ends `built` at once.
The planner's turns wait while the step runs, and the folder has one writer at a time: a mock holder's turn (the planner's, or the no-planner foreman's once the mock is open) waits while a baseline seat's turn of the card runs, and a baseline seat's while the mock holder's runs.
A stall in the step nudges the foreman.
A planner already at brief when the step ends, or a no-planner foreman that already received "Agree the specification" in this run, gets "The baseline is ready"; a no-planner foreman that has not yet received "Agree the specification" in this run gets "Agree the specification" once its baseline ends.

At brief the planner writes the mock beside its handoff, the rest of the clone read-only to it; a no-planner foreman keeps the foreman's wall and leaves tracked repository files untouched before approval.
The holder renders and inspects the baseline, then drafts the design the card specifies.
An open tradeoff about the direction, hierarchy or concrete changes, or a proposed departure, needs an answer before editing.
The operator answers that design ask, or the orchestrator on a self-sustained card.
The holder records answered decisions and renders the mock from its link before each recap.
It leaves the mock untouched while a recap is under challenge or waits for approve, editing it only once the operator's comment withdrew the recap or the foreman's objection came.
A visual recap is the plan and the mock together: one from a turn launched before the step ended is not offered, its holder told why; on a card with a planner the foreman's challenge request names the mock's link, or says nothing serves it, and the plan's `approve-brief` ask shows the sketch, then the link.
That approve is the operator's alone (`approve`): it ends the mock and asks the foreman for the build.
After it no mock is made but on the operator's reopen.
Turned on at brief with no mock or a skipped one, visual mode withdraws a waiting recap and opens the baseline step, asking the foreman as at start; a running turn finishes, its recap not offered.
Turned off at brief, the mock is skipped and the baseline seat released, its jobs stopped; a holder the step held is asked to go on agreeing the specification, and a delegated card's waiting plan is re-offered without the mock link so its ask is given automatically to the orchestrator.
The `visual-mocks` skill has how the mock runs.

The operator's `mock` reopens an approved mock in the same run until the final landing starts, withdrawing the submission and holding the build until its new approve.
On a card with a planner the planner mocks at once even while build turns run, the operator's words in the spec room reaching it and those in the card's room the foreman.
On a no-planner card the foreman mocks in the card's room, editing no tracked repository files and with `built` refused until the mock ends.
Its clear `brief-ready` offers the ready mock with its link in the hand's message, asking nothing: a `mock` ask with an `approve` button opens in its room, the operator's alone and never given.
Every turn of the mock holder takes the offer down and closes the ask, whatever woke it.
Its next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to it was left unread: that message already owes it another turn, whose `brief-ready` may offer the mock.
Approve is refused while the offer is down.
It makes each change he asks for and records it as a decision on the card.
On a card with a planner, the foreman neither mocks nor offers nor relays.
Turning visual mode off doesn't stop a running mock turn, and an offer made after the mock closed counts for nothing.
Before a card's clone is removed, its `.git/mork-mock/` is copied to `mocks/<card>/` in Mork's home, replacing an older run's copy; a failed copy holds the removal for `action.retry`, and a retry of the same run's removal keeps the copy it already recorded whole, and a card whose clone removal is held can't start a new run.
Once the next run's clone is made, the copy is put back in its `.git/mork-mock/` and dropped once the put-back is recorded.
A run whose clone never got the whole copy, its put-back failing partway, keeps the copy as it is when that clone is removed.
The next run's baseline seat keeps it and fills only what it lacks; its link may change.

The mock is the specification holder's work (the planner's, or the foreman's on a no-planner card), on its own seat, model and effort; its baseline the baseline seat's, on `[seats.baseline]`.
The retired `[seats.mocker]` is refused as a config error.

`[mork] vite` in mork.toml names a folder where the operator ran `npm install vite sass`: the engine serves the mock's folder with it on a second port of the card's, at `http://<card>.localhost:<port>/`, its output in `.git/mork-mock-host/host.log` in the clone, from its baseline step's opening until the card lands, closes or is done, or its mock is skipped.
No command starts or stops it.
Its host reads the clone and writes only its own `.git/mork-mock-host/`, where its configuration, cache and log live.
With no `vite` set, or no free port, the mock runs unserved: the engine's requests say so, and the specification holder raises it in its flagged hand message.
The card's seats get a line at their session's start with the mock's folder and link, and its review card's round request carries the link and says how the mock is judged.
Kept on the card across restarts and its later full runs; `card.show` says `visual`.
Answers `{"visual": {"card", "visual"}}`.
Refused for a card not in the state, a card that is not full, and a card that has ended.
The operator's alone (buzz): an orchestrator sets it only as it starts a card, with `start --visual`.

Example: `mork ctl visual ex-550`, `mork ctl visual ex-550 --off`

### auto

Turns a project's auto on, or with `--off` ends it: each new card that starts in the project begins at the first delegation level unless `start` names a level.
Cards already running keep their setting; turning auto off changes no card, and auto starts no card and caps none.

Each card's own delegation level governs whether seat `answer` asks and non-visual `approve-brief` asks are given automatically to the project's orchestrator without desktop notices, as `delegate` describes.
Mechanical needs go to the orchestrator as direct requests, and actionable engine asks on delegated cards remain the operator's while the engine requests the orchestrator's help with a message.
A non-visual plan ask given to that project's orchestrator lets it approve with `approve`, and the operator may approve it himself.
A new run keeps an enabled level (self-sustained included) unless `start` names its level; with none enabled, project auto starts it at the first level when on.
Kept across restarts.
`projects` gives each project's `auto`: `on`.
Answers `{"auto": {"project", "on"}}`.
Refused for a project not configured.
The operator's alone (buzz).

Example: `mork ctl auto example.com`, `mork ctl auto example.com --off`

### memory.read

A memory file's text: the memory is `general` or a project's name, the file one of its names.
A Dream seat's read leaves the usage tallies unchanged.
Any other seat's read is counted, and is the file's last read; an orchestrator's or the operator's answers the text and counts nothing.

Answers `{"memory.read": {"memory", "name", "description", "writer", "origin", "written_ms", "text"}}`, `writer` being who wrote it last and `origin` who created it.
Refused for an unknown memory or file.

Example: `mork ctl memory.read mork wall-tests`

### memory.search

Finds up to 20 committed files containing any whole query word.
Words are lowercase runs of letters or digits, and repeated query words count once.
BM25 ranks hits within the memories the sender can search.
Name occurrences weigh 3, description occurrences 2 and body occurrences 1.
Ties sort by memory, then file name.

A seat searches `general` and its card's project; an orchestrator or the operator every memory.
No read is counted.
Answers `{"memory.search": {"files": [{"memory", "name", "description"}]}}`.
Any sender.

Example: `mork ctl memory.search "door timing"`

### memory.write

Creates a memory file, or rewrites it: its `name` a slug of lowercase letters, digits and `-`, 64 at most, unique within its memory; its `description` one line of 160 characters at most; its `text` 4 KB at most.

A new name whose name and description words are close to an existing file in that memory is refused, naming that file.
Rewrite it under its existing name, or pass the optional boolean `--new` to create a separate file despite the match.
Rewrites of an existing name skip this check.

A rewrite keeps its reads counted and its `origin`, the label of whoever created the file; its `writer` becomes the rewriter.
Each applied write counts in the memory's `written`.
Answers `{"memory.write": {"memory", "name", "rewrote", "staged"?}}`.
A held write returns `staged: true`.

A live Dream foreman can edit its own project's memory immediately.
The engine refuses a name another writer changed or deleted during the run.
Each applied edit records its previous and new contents in the card journal.

The operator can edit any memory.
An orchestrator can edit its project's memory.
The configured Mork project's orchestrator can also edit `general`.
Any card seat can edit its own project memory during its closing turn, until a new round supersedes that turn.
During active work, file a lesson with `notes.add lesson`.
While the card is live, it holds these edits.
A linked review holds them on the reviewed card.
After the holding card ends, closing edits apply immediately.
A seat can rewrite or delete a file it just staged.
Reads, rankings, counters and session indexes show only committed files.
At card end the engine applies held edits in order.
An intervening write or delete, including another card's commit, preserves the newer change and skips the conflicting held edit.
The card journal records applied edits with their before and after contents and skipped edits with the proposed contents and reason.
Undo a creation with `memory.delete`.
Restore a rewrite or deletion with `memory.write` using the previous description and text.
Use `--new` if a similar name prevents restoration.

Example: `mork ctl memory.write mork wall-tests --description "Why wall tests skip inside a card's wall" --text "…"`

### memory.delete

Deletes a memory file: one you found wrong, or no longer worth keeping.
Answers `{"memory.delete": {"memory", "name", "staged"?}}`.
A held deletion returns `staged: true`.
The permissions, staging, conflict and journal rules of `memory.write` also apply to deletions.

Example: `mork ctl memory.delete mork wall-tests`

### memory.stats

The memories' counts and files, or one memory's.
Memories are listed with `general` first, then configured projects in their file order, then removed projects alphabetically.
Per memory, its files `written` (rewrites included), `read` and `deleted`, counted since its first write and kept when its files go, and `reviewed_ms`, the operator's last send to review in UTC milliseconds since the epoch, or null when none has been recorded.
Per file, its `writer`, `origin`, `written_ms`, `reads` and `last_read_ms`.
Only new seat reads update the read counts and last-read times.
Earlier values carry forward.
Files stay until removed with `memory.delete`.
Answers `{"memory.stats": {"memories": [{"memory", "written", "read", "deleted", "reviewed_ms", "files"}]}}`.
A read.

Example: `mork ctl memory.stats general`

### notes.add

Files a note to the project's orchestrator: `kind` is `feedback`, a bug, an issue or a suggestion about Mork or the project, `lesson`, what the sender learned that would save a later agent time on another card (a non-obvious how, a trap, a preference the operator stated, its text saying how it was checked), which its orchestrator triages into memory or a skill, `smell`, a code smell in code the sender's work doesn't change, or `proposal`, a grouped cleanup ready to become a card.
The `text` may not be empty.
A smell also requires one `--file`, `--line` (from 1) and `--why` it matters, `text` being what is wrong.
A proposal requires a non-empty, one-line `--title` and takes an optional `--file <path>` per file, in order, preserving commas in paths.
In JSON, the `file` field takes an array of strings for a proposal and a single string for a smell.
Only a proposal takes `--title` or repeated `--file`.
Only a smell takes `--line` or `--why`.
Feedback and lessons take none of those fields.
Any seat may file one; it goes to its card's project, with its card and seat recorded.
When `[mork] project` is configured, a seat's feedback or lesson about Mork, or a lesson for `general`, goes to that project's notes from any project's card with `--project <Mork's project>`, its card and seat still recorded; a smell or proposal stays under the seat's own project.
When `[mork] project` is unset, a seat cannot file cross-project notes with `--project`: every seat note goes to its card's own project.
The operator or an orchestrator may file one outside a wall with `--project <project>`, required there; its card and seat are then null.
An unknown project is refused, and a seat cannot name any project but its own or, when `[mork] project` is configured, Mork project for feedback and a lesson.
Answers `{"notes.add": <note>}`.
Journaled as `note.added`.
A note moves nothing, is no review finding and requires no fix; it stays until deleted.

Examples:

- `mork ctl notes.add smell 'Two copies of the parser' --file src/parser.rs --line 42 --why 'They can disagree on valid input'`
- `mork ctl notes.add proposal 'Reuse the shared parser to keep inputs consistent' --title 'Use one parser' --file src/parser.rs --file src/import.rs`

### notes

Lists a project's notes, newest first.
Any sender may read them.
Answers `{"notes": {"project", "notes": [<note>]}}`, each note `{"id", "project", "card", "seat", "time", "kind", ...}`, time in milliseconds since the epoch, `card` and `seat` null where none filed it, and by `kind`: `feedback` (filed with `notes.add`, or by the engine from a seat's closing turn, with its seat), `lesson` (filed with `notes.add`) and `survey` carry `text` (`survey`, an answer of the sampled survey kept from before the closing turn replaced it, also `from`, its seat's role and card); `smell` carries `file`, `line`, `what` and `why`; `proposal` carries `title`, `text` and `files`, an array of paths; `handoff`, filed by the engine as a card's clone is removed, carries the clone's `run`, `dir`, the folder in the project's records its handoff was copied to, `files`, each `{"path", "bytes"}` under `.git/mork/handoff/`, and `skipped`, the paths not copied: a link, or anything but a folder or a regular file.
A clone with no handoff folder, or one holding no regular file, files none.
A review card's handoff holds what its foreman left through every round.
A note whose project nothing accounted for is listed under Mork project when `[mork] project` is configured, and under no project when it is unset.
An unknown project is refused.
The window shows every project's in main's Notes tab.

Example: `mork ctl --as orchestrator:example.com notes example.com`

### notes.delete

Deletes for good only the named notes of the project.
The operator and orchestrators may delete notes.
A live Dream foreman may delete only its project's lesson notes.
A mixed selection is refused in full.
`ids` is a non-empty list of ids as strings, separated by spaces or commas in the CLI.
An id the project doesn't hold refuses the whole deletion.
So does a handoff whose card id is cited as a whole word, in any letter case, in the title, description or notes of any live card in any project (its stage is neither `done` nor `closed`).
The refusal names the note, the handoff's card and every live card citing it; it applies to every sender, the operator included, with no override.
No note is deleted or file removed on refusal.
Once no live card cites it, a handoff's note takes its copied files with it.
Answers `{"notes.delete": {"project", "deleted"}}`, counting distinct ids deleted.
Journaled as `notes.deleted` with who sent it.
A Dream deletion also records each lesson's ID and text on its card.

Example: `mork ctl --as orchestrator:example.com notes.delete example.com 12 15`

### orchestrator.review

Sends a project's orchestrator one section of main's tabs to review: `section` is `memories` (the Memory tab's memories) or `notes` (the Notes tab, the project's notes; `surveys` and `smells` name it too), and `target` a configured project, whose part of the section it is, or `general`, the Memory tab's shared memory alone, which goes to Mork project's orchestrator when `[mork] project` is configured.
A `target` that names no configured project, `general` with another section, or `general` when `[mork] project` is unset is refused without recording `reviewed_ms`.

The target project's stream gets a `review` line naming the section and the target, its `text` asking its orchestrator to review that part alone and report to the operator what matters: a memory by the orchestrators' skill section "A project's memory, notes and backlog", which holds the procedure, the project's notes read and cleaned up with `notes.delete`.
The line is read live while that orchestrator is hooked, else at its next hook's catch-up.
For `memories`, it records the target memory's `reviewed_ms`.
`memory.stats` reports the timestamp and the Memory tab shows its date.
It posts nothing in any place.
Answers `{"orchestrator.review": {"section", "target"}}`, journals `orchestrator.review`.
Refused for any other section.
The operator's alone (buzz): the orchestrators and every agent are refused.

Example: `mork ctl orchestrator.review notes example.com`

## Helpers

### mork login

`mork login claude|codex|agy` starts the CLI's sign-in in an interactive wall with `login-home/` as its home and working folder.
The limits reads use that home too.
For seat turns, login and limits reads, Mork sets `HOME` and `CLAUDE_SECURESTORAGE_CONFIG_DIR` and clears `CLAUDE_CONFIG_DIR` and `CODEX_HOME` after applying configured environment values.
For a personal Google subscription, run `mork login agy`, open the printed sign-in link and paste the code or landing URL into the terminal.
The wall has its own `/run`.
The agy token is stored in `logins/agy/antigravity-oauth-token` under Mork home.
Claude and Codex use `logins/claude/.credentials.json` and `logins/codex/auth.json`.
Each login remains writable so renewals reach the other seats.

### mork web

`mork web` reads `mork.toml` and `<home>/web.key` and prints `http://127.0.0.1:<web_port>/#key=<hex>` on stdout.
It never contacts the engine and never creates `web.key`.
It exits non-zero with one line on stderr when run inside a wall, when `web_port` is unset or `0`, or when `web.key` is missing or not 64 ASCII hex digits.

### mork test

Runs the tests of what you touched, in your clone, in a test slot: it waits for one.
Its first argument chooses what: `--run <program> [arguments…]`, `--file <path>…`, `--checks` or `--walls [--no-default-features] <file> [<filter>]`, each below.
`mork test --help` prints this.
A researcher's or baseline seat's run takes a project slot and a machine test run, but not its card's slot.

With none of them it is refused, with arguments or none.
The command's own output passes through; it exits with the command's code (0 a pass), 2 when it can't run (no wall, no Mork running, no clone, a suite refused, `--run` with no command, `--file` with no paths or no one-file command), 3 when the slot was lost, 126 when `--run`'s first word is a file that isn't a program (it says to put the program first), 127 when the command couldn't be started.
With no slot free it says `mork test: waiting for a slot (<why>)` and waits; across a reload, a restart or a new build of Mork it waits for it and asks again, then runs when granted a slot, for `--run`, `--file` and `--checks`.
While Mork is recovering it waits, asking again.
`--run` and `--file` run where you start them: run them from the clone's root.
The `testing-in-a-slot` skill says which to run when.

Example: `mork test --run npm test -- src/reader.test.js`

### mork test --checks

Runs the project's checks command on your clone as it stands, uncommitted changes and new files included, in a test slot: `mork test --checks`.
The checks command runs from the clone's root with the gate's arguments: `--base` the branch point of your HEAD with `origin/<base>` (in a clone with none, a review round's, the card's fork), `--json` its report, and no paths (paths given are ignored, and it says so).
The engine scans the submitted commits for known secrets when the card reaches checks.
Its checks folder's git checkout is mounted read-only in the wall (the folder alone when it is in none), so they can read their own commit.

The output names the checks command, the checks folder's commit and whether it is clean, then the checks' exit and the findings their report names; nothing it finds is kept.
A run the gate wouldn't trust (no report, a status that isn't the exit, exit 0 naming findings, exit 1 naming nothing, a finding outside the clone, any other exit) says why.
It exits 0 when the checks pass, 1 for findings, 2 for a run the gate wouldn't trust or a checks folder with uncommitted changes, which isn't run.
A project with no checks says so and exits 0.

Example: `mork test --checks`

### mork test --file

Runs the project's one-file command, `test_file` in mork.toml, a shell command the paths are appended to, in a test slot: `mork test --file <path>…`, from the clone's root.
Its output passes through, and it exits with the command's code; nothing is remembered, and verification never uses `test_file`.
A project with no `test_file` refuses it before taking a slot, and says to use `--run`.

Example: `mork test --file app/shared/bills/bill-add-directive.test.js`

### mork test --run

Runs a command of your own in a test slot, never the configured suite: `mork test --run <program> [arguments…]`.
Give the program and each argument as separate words, never the whole command as one quoted string (`"npx jest a.test.js"` is looked up as one program's name and not found), the program directly after `--run`, with no `--` before it; arguments after it pass through unchanged, a `--` among them included.
`{io}` in an argument becomes your seat's `io/` folder, the one `MORK_IO` names.

When `[mork] project` is configured, on that project it runs under its test settings: `env` with `MORK_UNWALLED=1`, and the app's `faults` added to a `cargo test` that builds the app and names no features of its own.
Its stdin and output pass through (a heredoc reaches the command), and it exits with the command's code; nothing is remembered: only verification's and the merge queue's passes are.

Example: `mork test --run cargo test -p mork-core --test reviewer_history`

### mork test --walls

Runs one wall-test file outside every wall, with its tests' walls enabled: `mork test --walls [--no-default-features] <file> [<filter>]`.
Only the foreman of a full card of the configured `[mork] project` may use it.
When `[mork] project` is unset, or for any other project's seat, review or report card's seat, researcher or other seat, `--walls` is refused before taking a slot.

The engine runs `cargo test -p mork-app --test <file> --features mork-app/faults` in the card's clone as it stands, uncommitted changes included, with `MORK_UNWALLED` unset.
`--no-default-features` before the file replaces the `faults` feature with the production build.
The file is a nonempty plain test-file name: ASCII letters, digits, underscores and hyphens, with no leading hyphen, path, extension or wildcard.
The optional filter is at most one nonempty word, with no leading hyphen, whitespace or control character.
Anything else is refused in core before a slot, including a direct `test.lease` request.
The seat names no program.

It waits for a slot, then waits for the engine's supervised job to end, prints its stdout and stderr and exits with its code.
A failed test raises no ask on the card, and no pass is remembered.
Closing the command's connection, ending the seat's turn, a reload or a restart stops the run; it never resumes or runs again by itself.
The slot is freed once the job is gone.
A lost connection exits 3: request the run again.
When `[mork] project` is configured, verification still runs both phases, and the suite remains refused to seats.

Example: `mork test --walls t1_walls t1_33`

### mork manual

Prints a manual of the running build: `mork manual <name>`, or the names alone.

`mork` is the Mork manual; `index` the API index; `reference` this whole reference; `contract` the reply contract; `planner`, `foreman`, `reviewer`, `researcher` and `baseline` each role's manual.
Every manual is open to every role.
It needs no running mork.

### mork ctl --help

Prints every command's signature; `mork ctl <command> --help` prints one command's signature and its reference.
