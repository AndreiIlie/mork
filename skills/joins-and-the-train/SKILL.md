---
name: joins-and-the-train
description: "Use when the base moved under a card (a join), or when a candidate is verified, waits in the merge queue or comes back: what its steps are, how a candidate is made and lands, how to merge the base in, and what comes back to the foreman when a candidate fails, conflicts or can't be trusted."
---

# Joins, verification and the merge queue

**Its entries.**
Our card's branch, once its head's checks pass or when none are configured; every pull request into a repository's base the poller reads, a colleague's, or ours at a head no verification passed; any pull request into any destination whose title contains its project's configured `forge.review_tag`; a pull request or branch the operator asked to review (`review`, `review.merge`, `review.branch`).
No `/merge` is waited for.
Each is verified on a candidate of its own as it comes, `merging` while it runs, none waiting for another: only its suite waits for a test slot.
A new head the poller reads on a pull request or a branch under review takes it through the steps again, on the same card; our card's new head takes it out, and passing its checks puts it back.

**Verification, in order:** its destination merged into the branch (`Merge <base> into <branch>`), then the project's checks, unless they passed on that commit; then, on that same commit, its review card's round and the whole suite side by side, neither waiting for the other.
The round starts from the files no passed round approved as it has them.
A pull request or branch starts from changes since its last reviewed source head, including failed rounds, rebases and reverts, with the whole change as context.
Reviewers trace fixes and merged base changes into affected callers and dependents.
The suite is skipped when the same tree already passed it under the same command.
Both results come back together once both ended: a failed round never stops the suite, nor returns the card before it ends.

**A candidate** is made and run in a disposable clone from the mirror's commits: the card's branch never sees it, and its agents may work on (a new head takes the card out).
Its run is the project's install and its steps, the commit and its tracked files read before and after, a change discarding the run.

**The merge queue.**
On a project without pull requests, our verified card enters its project's merge queue at the back and waits there, its agents needed for nothing.
One card at a time lands, the front, in the order they entered.
With the base where its candidate was made, its verified candidate lands as it is, run again for nothing; with the base moved, the new tip is merged into it and its checks and its suite run again, no round: passed, it lands.
When that merge changes files its rounds approved, it leaves the queue to be verified again, its round and its suite, and enters again at its back.
The card's turns are held from its landing step until it lands, unless the queue waits on the operator.
With `forge.backup` configured, each local landing also queues a non-forced push of the current base to that remote; its attempt may delay the next landing for up to two minutes, and failure leaves the card done with a held backup action.

**On a repository with pull requests** our card is verified the same way before anything is published, then published as the `pull-requests` skill says.
A colleague's pull request and a branch review are verified the same way off the queue, on their destination's tip, and report once their round and their suite both ended; a branch review only reports, and never lands.

**A join.**
"Join the base" reaches the foreman before the checks when its submitted head lacks the newest base tip the engine has seen, whatever files it changes.
It also comes when a peer lands on the card's files, the merge queue's merge of the base conflicts, or the card's pull request conflicts with the base, in its run or in the operator's approved merge.
Use `git merge <base tip>` with the message `Merge <base> into <branch>`, resolve the conflicts, commit, then run the checks and the tests of what the join touched (until the merge is committed, the checks read the old branch point, so the base's changes look like the card's), end `built`.
To check a join, compare per file `git diff -U0 <old base> <old head>` with `git diff -U0 <incoming base> <merge head>`: every remaining difference is a conflict resolution to inspect.
With several merge bases `A...B` picks one and the base's changes look new: diff named commits, `<previously reviewed head> <candidate>`, the head found from the merge's real parents.
The base's tip is in the clone as `origin/<base>` once the engine imported it, and as the local `<base>` beside it, which each import moves with it, so `git log <base>..HEAD` and `git diff <base>...HEAD` read what the gate reads (a review round's clone has neither, nor a local `<base>` beside a branch named so; a later round's clone holds the round before it as `refs/rounds/<n>`, and when the base moved and both candidates contain their bases may also project it onto the current base as `refs/rounds/<n>-on-base`, named in its request); a join request that names a tip the clone doesn't have is the engine's gap: send your project's orchestrator a request.

**What comes back.**
When our card's review round fails with findings and delegation is off, it waits for triage (along with any suite failure on the same candidate) instead of waking its foreman: `triage` closes each Decline finding, restarts verification directly when all findings are declined and the suite passed, or sends one fix request to the foreman carrying the Fix and Foreman decides findings plus any suite failure.
On a delegated or self-sustained card, a failed review round with findings skips triage and sends one request straight to the foreman (once any suite run ends) with every finding marked Foreman decides plus any suite failure, unless two consecutive completed non-superseded rounds under the current plan stall with none of the prior round's unresolved findings resolved and all repeating, which opens a red stalemate `triage` ask for the operator.
A failed suite with no failed review round, failed checks, or a failed round with no findings returns straight to the foreman; a conflict of its candidate returns as a join; the engine resolves none.
The same goes for a failure or a conflict at the queue's front.
Fixed and through its gates again, the card is verified again and enters the queue at its back.
A colleague's pull request's results are posted as the `pull-requests` skill says, a conflict as soon as it is known.

**What can't be trusted holds the card:** a suite that couldn't run (exit 126 or 127, a signal, an exit the project's `infra_exits` lists, such as a test database down), an interrupted run, a checkout that changed under it, checks that can't be trusted.
The card keeps its place, the queue's front holding the queue, a red ask says why, and only the operator's or an orchestrator's `resume` runs it again.
A stopped card steps out until its `resume`, which verifies it again.
