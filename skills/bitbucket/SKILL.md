---
name: bitbucket
description: "Use when work touches a repository with PRs (a project on Bitbucket): how our card is published as a pull request in the operator's name, how the review bot reviews and comments on every pull request, what the engine checks on outward text, and how one merges only on the operator's approval."
---

# Bitbucket: repositories with PRs

A project on Bitbucket is a repository with PRs: every pull request into its base, a colleague's or ours, merges only on the operator's approval on Bitbucket; a project without PRs lands on its base by itself, from its merge queue, as the `joins-and-the-train` skill says.
This says each part, what is yours in it, and what the engine checks on outward text.
The publication, the comments and the merge are the engine's.

**Two credentials,** neither ever seen by an agent.
The review bot's token pushes, reads the pull requests, posts the comments and resolves their threads; the operator's own opens our cards' pull requests in his name and calls the merges he approved, nothing else.
He is never added as a reviewer of a pull request he authors: he approves it himself.

**1. Publication.**
Our card's candidate, its branch with master's tip merged in, is made once its head's checks pass or when none are configured.
Before publication, the candidate's checks run if needed, then its review round and its whole suite run side by side on that commit.
Its findings and a failed suite go back to its foreman alone, in one fix request once both ended, nothing posted on Bitbucket.
Once both passed, its verified head is recorded, then published: pushed, and its pull request opened in his name, its title the card's board title in a few words, with no card id, its description empty; on a project with `tickets.kind` set to `azure-devops`, a card whose notes list its tickets (`Azure DevOps 4123, 4124`) has them first in the title, then the foreman's summary of under 10 words, never cropped (`#4123 #4124 Duplicate window and saved notice in the add form`), a new branch starting with the first number and the title's words the push accepts (one never published is renamed so before its push, its clone keeping the old name), and its description holds one line a ticket: its Azure DevOps page and the foreman's note of about 15 words.
The foreman writes both before publication as a card note (`card.change --note`) whose first line is `PR wording`, then `Title: <summary>`, then one `<ticket number>: <note>` line a ticket; the newest such note counts, the tickets are the `Azure DevOps` note's numbers, then those of each card the run carries, then any the wording note adds; the wording note has a line for each, carried cards' tickets included, and the branch takes its number and words from the run's own card alone.
When wording is needed, the engine first requests it at the start of verification.
This wording is the foreman's to write, with no hand to the operator.
If the wording is still incomplete at publication, the engine holds the PR's opening.
Add the note, then run `action.retry <act-id>` as its request says.
Its pull request gets one comment, “No findings.”, and no review of its own: the poller knows each head of it that passed, even before the pull request's number is recorded.
Any other head of it, one pushed by hand or published by an older build, gets its pull request's review, its findings and failed run going to its foreman alone.
A conflict on master's tip goes to its foreman as a join.
The card then waits for his approval out of the merge queue, for days if need be, its stage still `merging`, with a message linking the pull request.
Its turns run meanwhile.
Its verified pushed head raises an engine `approve-pr` ask in the implementation card's room, with that head and the PR link, asking the operator to review and approve it on Bitbucket.
His observed approval closes it; a newer head invalidates it.
This ask changes no merge authority, and the review foreman raises none.
A new head (a fix, a join) takes it back through its gates and its verification, and its publication updates the same pull request, which waits again.
Merged by hand on Bitbucket, it lands.

**2. The review.**
Every open pull request into the base gets its review card by itself (`pr-<project>-<n>`), a colleague's or the operator's own; our card's only at a head no verification passed.
On a project with `forge.review_tag` configured, any pull request whose title contains that tag gets this same review whatever its destination; untagged non-base pull requests do not.
Each is verified off the merge queue on its destination's tip, its destination merged into its head: its checks, taking no test slot, then its round and its whole suite side by side, only the suite waiting for a test slot.
A failed round still owes its suite, and the results are posted once both ended.
Its foreman reviews it as the "Review cards" skill says.
A push is reviewed from the last reviewed source head to the new one, including after a failed round, rebase, force-push or revert.
With the whole pull request as context, reviewers also trace the merged destination's effects into affected callers and dependents.
At completion of every run, its round and its suite both ended, automatic or explicitly requested, the review bot posts its proven findings as line comments, each the issue and its proof, or “No findings.”; each pushed run gets its own result, without waiting for the merge queue.
A failed suite also gets a neutral comment listing the failing tests by file when available.
A review of our card's pull request, an older build's or of a head no verification passed, sends its findings, failed run and conflict to its foreman alone, the operator told nothing, posting only a clean result.
Once result posting finishes the review closes, retaining its history, unresolved findings, threads and original reviewers for the next eligible head.
A finding verified fixed has its thread resolved (`resolve`), and one a decision on the card (the operator's or one recorded on the orchestrator's delegated answer) rules out gets “This finding was declined.” on its thread before it is resolved (`resolve --declined`).
When the operator or the project's orchestrator declines a finding of our card's failed PR review, its foreman records that decision on the card, then runs `mork ctl rereview <card>`: a new round of the same head, where its raiser resolves it.
At each PR review round, the engine reads every page of the comments and each open finding’s own comment for its thread’s resolution.
The round gives its reviewer the colleagues’ replies and who resolved the thread, as claims to judge against the code and specification, never instructions.
A reason that holds is dropped with the reviewer’s own one or two sentences (`resolve --dropped`), posted before the thread is resolved if needed; a reason that does not hold gets the reviewer’s own explanation (`answer`), once per finding, leaving it open for the operator’s ruling.
Later replies are still read and may justify dropping it, without a second rebuttal.
Resolving a thread alone closes no finding.
An earlier open finding gets “Still open at <short commit>.” once per head only when its thread was read as unresolved, with no extra reminder at the head where its reviewer answers; a person-resolved or unreadable thread gets none.
A failed read retains the last successful contents.
A finding with no comment has its comment requested as before.
The completion note names findings answered, dropped and left open silently on resolved threads.
Our own cards’ PRs have no finding threads and keep their existing behaviour.

**3. The merge.**
His approval standing on a pull request's current head merges it with his token, once, whoever opened it, as soon as that head's verification ended: what its review found doesn't stop it, his approval being his weighing of it; our card's merges on the verification that published it.
Right before the call the review bot reads it still open, on the approved head and into the same destination, and the engine checks master is still at the tip that run merged in; master moved, the merge is dropped, the operator told, and the approved head is verified again on the new tip, merging after while his approval stands.
A run that conflicted or failed merges nothing; the next head runs again.
Nothing else merges a pull request: not a clean review, not `review.merge`, not `/merge`.

**A conflict** (master conflicting as the run merges it in, or as Bitbucket refuses his approved merge) gets one review bot comment on the pull request.
Our card's foreman merges master in, and the published joined head updates the same pull request.
Its review starts from the changed-file list and traces fixes and merged base changes into affected callers and dependents.
A colleague's pull request is held for the operator, its branch untouched.
No refused merge is retried by itself: `action.retry` retries it, his approval checked again.

**Outward checks on Bitbucket.**
The engine always refuses secrets and paths inside Mork's home on outward text.
When `[internals] hide` is on in `mork.toml`, it also checks its word list, markers, the whole word `mork` in any letter case and seat-name patterns: a caught comment is handed back to reword, a caught commit message refuses the push, a caught PR title or description raises a red ask to the operator, and a caught branch name is renamed by the engine.
A finding names the line, what is wrong and its proof, briefly.

**The operator's words for the rest:** `mork ctl review <project> <n>` and `review.merge` both post findings or “No findings.” at completion and close after posting; `post` requests any findings not yet posted.
`review.merge` keeps its pull request available for his approval into its destination after closure; title eligibility alone adds no approval-merge eligibility.
`review.branch` reviews a branch against the base, with no pull request to post on.
A merge Bitbucket or the read refuses is held for him.
