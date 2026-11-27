---
name: delegating
description: "Use when a foreman tasks a reviewer or baseline seat, or a planner or foreman tasks a researcher."
---

# Delegating

The foreman builds every deliverable.
Researchers and reviewers take questions and review topics.
Use the least work that answers the task.

- **Spawn a researcher, reviewer or baseline seat.**
  Use `mork ctl spawn <card> researcher '<task>' --model <model> --effort <effort>` for a researcher, or `spawn` with `reviewer` on a review card.
  For a task with quotes or parentheses, its `--task-file <path>` option reads the task from a file in place of `'<task>'`, `-` reading stdin.
  A visual card's `baseline` seat is the foreman's at its baseline step alone, one at a time: `mork ctl spawn <card> baseline '<task>'`, naming no model, effort or CLI, on its configured default; only the operator picks it later.
  Task it with the pages to reproduce as they stand today, the mock's link, and "keep what the folder holds, fill what it lacks"; end `built` once its report says the baseline is done.
  A planner can spawn researchers at brief or during a specification dispute.
  The planner is no child: a foreman neither spawns nor picks it.
  A researcher spawn requires both model and effort.
  A reviewer requires `--topic '<topic>'`, quoted, what it reviews in your own words, and a researcher takes none.
  A reviewer may omit model, effort and CLI together to use its configured inside or outside default, without allowlist permission.
  For an explicit selection, name both model and effort from your Allowed models section.
  Selections also preserve the seat's wall: an unwalled Claude seat runs Claude models only.
  `--model` takes the full id or its label's first word; name `--cli` to pick another CLI than the model's own, or when the model has no default CLI.
  A refused spawn makes no child.
  It answers its name (`researcher-2`, `reviewer-1`) and its DM (`<card>/<child>/dm`), where your task is its first request.
  It is walled, has a session of its own, and shows nested under you in the window.
- **Scout with researcher seats** on a model its list allows.
  Read the card and what it names yourself.
  Send the rest as bounded questions the plan depends on, with no fixed roster or count.
  You may skip scouting when the card already names the place.
  The CLI's own subagent tool stays as it is and is not a scout.
- **Pick a child's model or effort.**
  The pick applies from the child's next turn and uses the same permissions for its role and review kind as an explicit spawn, even when a reviewer began on its automatic default.
  A model or effort change on the same CLI resumes the session.
  A CLI change starts a fresh session.
  Seat defaults and foreman permissions are separate: `[seats.researcher]` sets researchers' defaults, and `[seats.reviewer]` has `inside` and `outside` settings for our work and outside work.
  Each setting may choose any CLI, model and effort, without constraining or granting a foreman's selections.
  - For a bounded research question, choose a model at the highest effort on the researcher's allowed list.
    For a short "where is X" lookup, choose a model at the lowest effort on that list.
  - **A reviewer** spawned without a selection takes its configured inside or outside default.
    Choose the starting effort for the round as the `review-cards` skill says.
    For any topic that touches money, permissions or migrations, use a stronger allowed model at that effort from the start when available.
    For a topic where a reviewer left a doubt unresolved, use a stronger allowed model at that effort or higher.
    If none is available, use a higher allowed effort on the default model when possible.
  - Going up, for a reviewer alone: when a reviewer's work comes back wrong (wrong logic, a missed case, a lost requirement; never for style or scope), take the next allowed step on its current selection: a higher allowed effort, else a stronger allowed model; with neither left, it stays as it is.
  - Equal fit: the lower effort, then the cheaper model.
- **A good task names the question or review topic.**
  Give the relevant files or behaviour, the runs the Mork manual allows, and any decisions the child needs.
- **Steer a running child:** your ordinary message to it, a reply's or a `say`'s, reaches its running turn between its tool calls.
  A child on agy or unwalled Codex reads the message on its next turn.
  Send now, `mork ctl say <card>/researcher-1/dm --kind request --delivery now '<text>'`, cuts its turn and drops its reply and continues its session with your words, after it checks what already happened: for a child heading the wrong way.
  Only the command can; a reply's `messages` can't.
- **One writer at a time.**
  The foreman writes the card's working tree.
  Researchers work in folders of their own, the clone theirs to read only (`$MORK_CLONE`).
  A baseline seat writes the clone's `.git/mork-mock/` alone, and it and the card's mock holder (the planner, or the foreman on a no-planner card once the mock is open) take turns there.
- **End your turn when only the child's work remains.**
  As foreman, after you send a task in your reply, with `say` or `spawn`, end `working` as soon as your independent work is done and the child's report is still outstanding.
  Never wait or poll for it in that turn.
- **Its report is no submission.**
  A planner's researchers wake it each time.
  If you read a report during independent work, act on it in that turn.
  A report your turn ends without reading wakes you then; messages the child writes itself to you stay queued for your next turn.
  The report moves nothing.
  Read its report against the task before acting on it.
  Ask it to check again if the task needs more.
- **A child's question** comes to you as a request.
  Answer it from the card's specification; an open specification question goes to `planner` on a full card that has one, else in your flagged hand message under the card's delegation level, then tell the child.
