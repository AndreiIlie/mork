---
name: dream
description: "Use for a project's Dream pass to verify and consolidate project memory and fold lesson notes."
---

# Dream

Read the card and its input file for the project, fixed commit, previous Dream cursor and file budget.
List the project's files with `mork ctl memory.stats <project>`.
Compare each file’s last write with the memory’s reviewed_ms to identify changes since the last operator review.
Use writer and origin to trace the lesson, and reads and last read to prioritize pruning.
Read each file with `mork ctl memory.read <project> <name>`.
Reads by this run's seats leave usage tallies unchanged.

On an auto-started pass, omit model, effort and CLI at spawn.
The engine chooses from `picks.pass` and refuses the spawn if no pool is open.
Give researchers groups of about fifteen files to check against the fixed commit.
Ask for a verdict of true, stale, contradicted or duplicate, with a file and line for each code claim.
The foreman alone edits.
Rewrite stale entries that can be made accurate and delete contradicted entries.
Merge duplicates into a surviving file before deleting the others.
If the engine reports a name changed during the run, leave it for the next Dream.

Read the project's lesson notes with `mork ctl notes <project>`.
Verify a sound lesson, fold it into project memory, then delete the note with `mork ctl notes.delete <project> <id>`.
Delete a lesson already covered.
Leave lessons intended for general memory or a skill and name them in the report.

After folding, prune verified files to the budget, least read first and longest unread first among ties.
Keep every edit within project memory.
Leave code, manuals, skills and cleanup proposals to Pulse or Deep Sweep.
File no proposals, smell notes or feedback notes during the pass.

Report what you merged, rewrote, deleted, folded or left, including anything you could not verify.
End with `report-ready`.
The card journal keeps each edit's previous contents for restoration.
