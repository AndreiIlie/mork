---
name: the-door-and-databases
description: "Use when your work needs something on a machine outside your wall, a database above all: how to find a target and what it allows, run a read-only query or EXPLAIN on a replica through the door, and read its server_us against elapsed."
---

# The door and databases

Use the door for the database and machine operations described here: it runs one operation on one target over Andrei's own access.
No credential, host or alias is ever shown to you, and its database operations are read-only.

Calls run one at a time per target, in ask order.
A queued request prints a waiting line on stderr, and its window step says it waits: the running call's action id, requester, op, elapsed milliseconds and time limit, and how many calls wait ahead, never its query or parameters.
Each waiting request has its own 60 second limit before its call starts.
At that limit it answers `busy`, whose `why` carries those same facts, says nothing ran and says to ask again.
Requests sharing one call each get their own waiting line and limit; the call is dropped unstarted once its last waiter times out.
A queued read (every op but `sh.run` and `misc.run`) is also dropped at once when its last caller disconnects (a restart included); a started call keeps running, except `sql.select`, which stops when its last caller disconnects.
A busy or caller-gone call answers no later request: asking again, in the same turn too, makes a new call.
A non-read call nobody waits on, including an approved `misc.run` or a `sh.run` pending across a restart, has no queue time limit and runs when its target is free.
Direct calls never post news when disconnected (only asked and held calls do); retrying in the same turn still gets a finished call's result.

1. **Find the target.**
   `factory ctl door.help` lists the targets, their services, reads, logs and limits; `factory ctl door.help <target>` describes one, and says which is a replica (`sql.read_only_server`; `read_only` is true on every SQL target).
   `factory ctl door --help` lists the operations (`sql.select`, `mongo.find`, `file.tail` and the rest), `factory ctl door --target <t> --help` a target's.
2. **Ask for what the target allows.**
   A request is resolved against the target's allowlist.
   An allowed call runs directly.
   A `misc.run` becomes Andrei's ask in the build room on a two-room card and in your place otherwise.
   The news in your place tells you how it went.
   Other calls outside the allowlist are `not allowed`.
   If the `failed` text says the query may still be running on its target, its server didn't take the kill: the same query is `not allowed` until it has ended there (each ask tries the kill again; the `why` says when it clears).
   Your other queries run meanwhile.
   Invalid requests, unsupported operations and writes are refused.
   Don't ask for more than the work needs, and don't work around a refusal.
3. **Query a replica** with:

```sh
factory ctl door --target <replica> --op sql.select --params "$(cat <<'JSON'
{"query": "SELECT id FROM app.users WHERE name = 'O''Neil'"}
JSON
)"
```

   Every target takes any read its login may run, as written, with no table list, except `information_schema` and the `sys` schema: one `SELECT`, `WITH`, `TABLE`, `VALUES`, `SHOW`, `DESCRIBE` or `EXPLAIN`, escaped and multiline strings, optimizer hints and executable comments included.
   A write under `EXPLAIN` stays unmeasured.
   The session is read-only on the server.
   Backslash escapes and multiline literals use a consistent connection mode; hints cannot change that mode or the row cap, or lift the time limit.
   The server ends a read at the target's timeout, and kills it when its call ends early: if your `factory ctl door` is killed (a tool's own timeout) or your turn is cut, its query goes with it, so keep a long read's wait alive (run it in the background).
   A LIMIT larger than the target's row cap is lowered to it before the query runs.
   A replica differs in its bounds: no row cap but its own, a longer timeout.
   Write real values into the query, its only field (it takes no bound parameters), and a LIMIT when you don't need every row.
4. **Read the timing.**
   The answer carries `elapsed`, the call's milliseconds with its ssh and database connections included, and for a database `server_us`, the query's own time on the server in microseconds.
   Judge a query's cost from `server_us`.
   A null one is unmeasured, never cheap (a failed query, a SQL client cut at its size cap, a server without profiling, every Mongo call): judge nothing from it or from `elapsed`, and say it was unmeasured.
   A SQL client's output cut at the target's size cap carries `server_us_why` beside its null `server_us`, saying the answer was cut at that cap before the query's time could be read from its server.
   A completed read trimmed afterwards keeps its time.
5. **Say what you ran** and what it returned in your hand, with its timing.

`mongo.find` takes any collection the configured read-only login may read, with server-supported filters including `$expr`.
Keys such as `$where`, `$function` and `$accumulator` inside literal documents and values are data.
Projection, sort and document/byte/time bounds apply.

`file.get` copies one file under a target's named root, bytes intact, into your seat's `io/` folder (`FACTORY_IO`), up to 20 MB, with `file.cat`'s guards and no ask: `factory ctl door --target production --op file.get --params '{"root": "upload", "path": "ap_<idAssociation>/docs/<n>/bills/<id>.pdf"}'` answers `file`, its path, and `size`.
Read it there with your own tools (`pdftotext`, `unzip`).
`door.help` lists the file roots; build the path under a root from the database.

`libra.get` takes any GET path `/…` at the configured API origin, with no ask.
Optional endpoint names are aliases/templates: parameters fill `{name}` slots and all extra keys and values are encoded into the query.
Credentials and tokens stay with the engine, requests stay at the configured origin, and signing stays on the server.
Output masking and limits apply.
File roots and named logs keep their location restrictions.

**A project's data folder** needs no door: on a card whose project sets one (`data` in factory.toml), it is in your wall at `/data`, read-only, its datasets one subfolder each, read with your own tools (`grep`, `pdftotext`).

`factory manual reference` has the door's whole reference (`door`, `door.help`).
