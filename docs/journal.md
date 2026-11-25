# Journal ordering

## Time

The engine reads the clock once for a decision, passing the same `now` to core as
`ctx.now` and to the commit ([runtime/mod.rs: `Engine::apply_with`, `decide`,
`commit_decided`](../crates/engine/src/runtime/mod.rs)). Every journal row in that
transaction carries this time ([store/mod.rs: `Store::commit`](../crates/engine/src/store/mod.rs)).
The job's `settled_ms` written by the decision uses it too
([agents/ends.rs: `settle`](../crates/core/src/agents/ends.rs)), as does a new
approval's `at_ms` ([lifecycle/brief.rs: `approve`, `offer`](../crates/core/src/lifecycle/brief.rs)).

## Rows and order

The journal is the `events` table. Its `seq` records insertion order; read it in
that order, rather than sorting by time or assuming causal order
([store/schema.rs:17](../crates/engine/src/store/schema.rs#L17),
[store/journal.rs: `Store::journal_rows`](../crates/engine/src/store/journal.rs)).
The rows of a decision are exactly its final `Decision::journal` vector, inserted
in order alongside its record changes in one transaction
([store/mod.rs: `Store::commit`](../crates/engine/src/store/mod.rs)).

Within a call to [`Poster::finish`](../crates/core/src/post.rs), rows are appended
to the decision in this order: `message` rows in posting order, `ask` rows in ask-ID
order, `card.stage` rows in card-ID order, then the Poster's collected journal rows
in the order their handlers wrote them. A `card.stage` row is added when the card
was absent from the input state or its stage or pause flag changed (same function).
It carries the run number at that transition.

This is not a universal prefix for a decision. Direct calls to
[`Decision::journal`](../crates/core/src/decide.rs) append where the caller places
them: [`material`](../crates/core/src/maintenance/material.rs) writes `material`
before calling `Poster::finish`, and
[`switch_line`](../crates/core/src/agents/models.rs) writes `pick.switch` after it.
Some handlers build a decision without a Poster at all:
[`running`](../crates/core/src/agents/turns.rs) writes `job.running`, while
[`engine_says`](../crates/core/src/messages.rs) stores a message but journals
`engine.says`, without a `message` row.

After `route`, [`decide`](../crates/core/src/decide.rs) calls
[`streams::record`](../crates/core/src/messages/streams.rs), then
[`delegate::give_opened`](../crates/core/src/messages/delegate.rs), then
[`delegate::request_opened`](../crates/core/src/messages/delegate.rs). The first
appends orchestrator-stream records without journal rows. The second automatically
gives each newly opened `answer` ask and non-visual `approve-brief` ask on a
delegated card: it drops `Intent::Notify`, sets `notice: false` and `given: true`,
appends a `given` stream record and journals `give`. The third appends a request
message and its stream record for each actionable engine ask newly opened on a
delegated card, and journals each as a `message` row after the route's rows,
without marking it given or journaling `give`; a command closes it as before.
The `delegate` row keeps `card`, `delegated` and `self_sustained`.

When a bundle requests a new take, its `action` row precedes the bundle's
`job.settled`: [`job_ended`](../crates/core/src/decide/jobs.rs) calls
[`bundle_ended`](../crates/core/src/lifecycle/gates.rs), which requests the action
through [`Poster::act_as`](../crates/core/src/post/actions.rs); `job_ended` then
collects `job.settled`. Any `card.stage` emitted by that same `Poster::finish`
precedes both.

## Reading a card's history

`review.answered` and a `review.resolved` row with `dropped: true` name the
reviewer's reply action. They record its accepted request; `action.done` records
its posting. Words rejected before the request is recorded emit neither row
([reviews/responses.rs](../crates/core/src/reviews/responses.rs)).

A submission that would start the checks requests a fresh take, even when an
earlier bundle recorded the same head. `built` requires a clean tree and a
committed change, leaving an unchanged head already at review or beyond in place.
If the fresh take's branch point differs from the mirror's base tip, a full card
imports the tip and asks its foreman for a join before the checks. The joined head
returns with `built` and is taken afresh
([lifecycle/gates.rs: `bundle_ended`, `built`, `taken`](../crates/core/src/lifecycle/gates.rs)).

The card stores only its latest approval, replaced when the brief is agreed
again ([lifecycle/brief.rs: `approved`](../crates/core/src/lifecycle/brief.rs)).
Its `brief.approved` row's `by` names Andrei or the card's project's orchestrator
whose `approve` is accepted only while its open plan ask is given to it. A
delegated non-visual plan no longer approves itself: `brief::offer` always raises
`approve-brief`, and `delegate::give_opened` gives it automatically so the
orchestrator or Andrei approves it with `approve`. Turning `delegate` on while a
plan or seat ask waits gives those open ungiven asks (journaling `give`) without
approving the plan; switching from delegated to self-sustained re-emits `given`
stream lines for open already-given seat and non-visual plan asks. A visual card's
plan is approved by Andrei alone, so its row's `by` is always his
([lifecycle/brief.rs: `offer`](../crates/core/src/lifecycle/brief.rs),
[messages/streams.rs: `give`](../crates/core/src/messages/streams.rs),
[decide/buzz.rs: `approve_buzz`](../crates/core/src/decide/buzz.rs)).
Orchestrator approvals are announced as `orchestrator:<project>, on Andrei's
delegation,`. Completed cards' older rows recording planner or foreman approvals
remain readable in the journal.

Older builds' `brief.planner-joined` rows recorded the startup transfer of a live
brief to its planner. Earlier agreements remain in `brief.approved` and older builds'
`brief.clear`, read against `card.agreeing` and `mock.opened` rather than inferred
from today's approval ([lifecycle/brief.rs: `approved`](../crates/core/src/lifecycle/brief.rs)).
A visual card's mock opens at its baseline step when its clone is made, or when
visual mode turns on at brief: `mock.opened` with `baseline: true` and its `port`
([lifecycle/baseline.rs: `open`](../crates/core/src/lifecycle/baseline.rs)). The
foreman's `built` that ends the step emits `mock.baseline-ended`, the baseline
seat retiring through the existing `closing.owed` and `closing.ended` rows when owed
a closing turn ([lifecycle/baseline.rs: `built`](../crates/core/src/lifecycle/baseline.rs)).
A visual recap from a planner turn launched before the step ended, or made during
it, is not offered: `brief.withheld` with `baseline: true`
([lifecycle/baseline.rs: `recap_withheld`](../crates/core/src/lifecycle/baseline.rs)).
The plan's approval ends the mock in the same transaction, `mock.ended` with step
`approved` after `brief.approved`, and asks for the build. Visual mode turned off at
brief emits `mock.ended` with step `skipped`
([lifecycle/baseline.rs: `turned_off`](../crates/core/src/lifecycle/baseline.rs)).
Older builds' rows stay as they were: their plan's approval opened the mock, with
`mock.opened` before `brief.approved`, and the mock's own approve emitted
`mock.ended`; a reopened mock (`mock`) still does.

The strip reads `brief.planned` as the first specification plan's start, not the
deliverable task lists' `plan` rows. It stays under plan through corrections and the
one challenge, until `brief.approved`. Older waiting plans start at their
`approve-brief` ask's opening, so withdrawing the offer keeps the strip under plan.
`mock.opened` and `mock.ended` delimit mock, but a `mock.opened` with
`baseline: true`, whose mock is made at brief, which marks nothing; older builds'
`mock.deferred` rows, which closed a mock at brief, end it as `mock.ended` does.
Checks' stage time counts under review, without changing the engine's checks order
([views/timeline.rs: `points`, `totals`](../crates/core/src/views/timeline.rs)).

`challenge.asked` records the request sequence for the foreman's challenge. `challenge.answered` records that sequence and whether it raised objections.
Older journals retain their `required` rows
([agents/required.rs](../crates/core/src/agents/required.rs)).
The retired journal writer recovery no longer selects review vendors; reviews
read their inside or outside settings from configuration.

When `taken` accepts a moved head on a live card, the gates' results are reset to
defaults while `clean` and `built_pending` are preserved. Current gates therefore
do not retain the earlier head's results
([lifecycle/gates.rs: `taken`](../crates/core/src/lifecycle/gates.rs)).

A job settled `applied` is no accepted reply. A first refused reply settles
`applied` too, owing a correction: its `job.settled` row's `how.outcome` is
`applied`, beside a `reply.refused` row that carries no card. A closing turn's
first refused reply stores the same settlement on its job but emits only
`reply.refused`, with no `job.settled` row. Find a refused reply by its agent and
time, not by card
([agents/ends.rs: `ended`, `apply_reply`](../crates/core/src/agents/ends.rs),
[agents/closing.rs: `ended`](../crates/core/src/agents/closing.rs),
[types/state.rs: `Settlement`](../crates/types/src/state.rs)).

In older journals, a builder's `launch` row names its deliverable, its agent, whether it spawned
the builder and whether a compaction is owed. That compaction's `job.intent` and
`job.settled` rows carry `compaction: true`: no reply is read from it, so its
`applied` says only that the session was compacted, and any other end but a usage
limit posts one update to the foreman instead of an ask, as does a `stop` of the
builder or its card during that limit's wait, unless a `brief` dropped its launch
meanwhile. Its `cost` row's `delta` and `tokens` are its own usage: Claude's
result's, or, as its thread's totals leave it out, the sum of the `token_usage_record`
lines Codex's saved thread holds for the compaction's turn, each response once; with
none read, `unknown` is true and no `tokens` are given. `total` stays the session's.
