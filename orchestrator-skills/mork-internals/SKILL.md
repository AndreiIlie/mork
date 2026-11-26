---
name: mork-internals
description: "Use when you run the Mork instance itself, as Mork project's orchestrator or the operator: before changing mork.toml or targets.toml, building, pinning or reloading, switching or renewing a Claude account, upgrading a seat CLI, changing a project's checks, dropping a project, checking machine load, or cleaning up Mork's disk. Driving cards is the mork-cards skill."
---

# Running Mork

When `mork.toml`'s `[mork] project` names the project holding Mork's own repository, its orchestrator runs Mork itself, and other projects' orchestrators ask it for changes here.
When it is unset, the operator does, and `build`, `pin` and `reload --target` are refused.
For a command's syntax and who may send it, run `mork ctl search <query>` or `mork ctl <command> --help`; the configuration keys are under Configuration in `mork manual reference`.

Mork's home is `$MORK_HOME`, else `$XDG_DATA_HOME/mork`, else `~/.local/share/mork`.
It holds `state.db` (read it, never write it), `jobs/`, `clones/<card>/`, `mirrors/`, `records/`, `builds/`, `logins/`, `login-home/`, `homes/` (each seat's home), `targets.toml` and `logs/app.log`.

## Config and accounts (never print a secret)

- **`mork.toml`** (`$MORK_CONFIG`, else `~/.config/mork/mork.toml`) is the live truth for `[mork]`, `[seats.*]`, `[[picks.*]]`, `[projects.*]`, `[slice]`, `[claude_accounts]` and `[tunnels.*]`.
  Read it live rather than trusting prose.
  Back it up before every change (`cp -a mork.toml mork.toml.bak-<date>-<what>`).
  An unknown or retired key fails startup and reload.
  Never print `web.key` or the keyed URL `mork web` prints.
- **`targets.toml`**, in Mork's home, holds the door's targets: never read or print a credential's value.
- **Claude accounts:** the active login is `logins/claude/.credentials.json` with `login-home/.claude.json`; saved accounts are `login-home/.claude/accounts/<name>.credentials.json` and `<name>.claude.json`, each named in `[claude_accounts]`.
  Switching accounts is the operator's call (`claude.account <name>`, or a pool's header in the window's Usage section): never switch, nor offer to.
  Ask him to sign in again only when a login hits its hard expiry.
  A new sign-in or renewal runs in a temporary folder (`HOME=<folder> claude auth login`); copy its `.credentials.json` (mode 600) and `.claude.json`'s `oauthAccount` and `userID` into the saved account's files, then delete the folder.
- **Seat CLIs:** `claude`, `codex` and `agy`.
  After a CLI upgrade, run a turn of a seat on it and check that it ends and `mork ctl usage` still reads its pool.
- **Machine load:** every job runs in `mork_work.slice`, its CPU and IO weights and `memory_high` set in `[slice]`.
  Check a project's `test` command before blaming the machine.
- **A project's checks:** `checks = { command, folder }` runs `command` in `folder`.
  To ship a change to a checks tool kept in its own checkout, make a new checkout of the new commit, point `folder` at it (backing up `mork.toml` first) and reload.
- **Dropping a project:** delete its memory and notes while it is still configured (`memory.delete` and `notes.delete` refuse a project that isn't).
  Then remove its `[projects.<name>]` section, and delete `mirrors/<name>`, `mirrors/<name>.bundles` and `records/<name>`.
  Remove a config key a card retires before that card lands.

## Builds, pins and reloads

- Never reload or restart Mork without the operator's explicit word: tell him a build is ready instead.
- Mork builds its base branch's new tip by itself and pins it; `build <commit>` builds one by hand, into `builds/<commit>/`.
  Every pin and every completed reload points `~/.local/bin/mork` at its build.
- The running build is the `runs <commit>` of the last start line in `logs/app.log` (`tail -10`), never the `~/.local/bin/mork` link, which follows the pin.
- To apply a config change or a new build: `mork ctl --as orchestrator:<project> reload [--target <commit>]`, then wait until commands stop answering `reloading`, and start `wakes` again.
- When a card changing `web/` lands and `[mork] web_assets` names the operator's checkout's `web/dist`, rebuild it there with `cargo xtask web`.

## Notes, manuals and disk

- **Notes:** check Mork project's notes (`notes <project>`) against the current code, as the mork-cards skill says, deleting each once handled.
- **A manuals pass** (on the operator's word, cuts only): run a `report` card auditing `manuals/`, `roles/`, `skills/` and the session-start prompts against the code (rules the engine already refuses, duplicates, lines about a single incident).
  Bring him every candidate cut one at a time, then land the approved cuts in a normal card.
  Never run it while another card rewrites those files.
- **Disk cleanup** (on the operator's word): measure first (`du -sh <mork home>/*`).
  Delete only finished job folders (those holding `exit.json`) and bundles older than 120 minutes (`find <mork home>/mirrors/*.bundles -maxdepth 1 -name '*.bundle' -mmin +120 -delete`).
