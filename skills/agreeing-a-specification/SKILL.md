---
name: agreeing-a-specification
description: "Use when you hold a card's specification and must settle it before anything is built: a full card's start. The method, delegation levels, and how a decision is saved on the card."
---

# Agreeing a specification

The planner settles a full card's specification at brief and on named requests after approval.
On a full card started with no planner (`start --no-planner`), its foreman holds the specification at brief and after approval.
Before approval that foreman writes, edits and commits nothing, except for edits in `.git/mork-mock/` after a visual card's baseline step under step 8.

1. **Read the card first** (`mork ctl card.show <card>`) and what it names.
   **Scout the rest.**
   Give researcher seats the other questions the plan depends on: the code and data's facts, what to reuse and the house style, outside documentation, best practices, and whether several paths do the same business thing.
   How many and which questions are yours, with no fixed roster, and you may skip scouting when the card names the place already.
   The `delegating` skill has how to spawn and task them.
   Treat the proposed solution as draft zero.
   Identify the underlying problem and use your judgment and the scouts' evidence to question the premise.
   Could a stronger design handle a case the proposed solution misses, even if it costs more to build?
   Could a simpler design remove the need for the proposed mechanism?
   Could other callers use the same solution?
   Consider a moonshot when it offers a concrete advantage.
   A proposal needs no cited source: the operator filters the weak ones.
   Establish what the evidence establishes; never ask the operator for it.
   Judge code questions by these three tests: 1. Modules built so that others sit on top of them: reusable, scalable.
   2. The simple approach (the hardest of the three to judge).
   3. A tiny feature that costs a lot of RAM or CPU is a candidate to drop.
   The bar chooses between solutions and adds no scope: the top rule still bounds what is built.
2. **Build on the card's words.**
   Its title and description are the issue and the operator's words, whoever typed them; they and his explicit decisions, plus decisions settled on his delegation by the project's orchestrator answering given asks, are authoritative.
   Build on them and challenge an earlier decision by raising a question in your flagged hand message (never deciding a change alone).
   A solution the text names is his idea, open to that same challenge.
   A note that is not a decision stays a proposal.
3. **At brief, raise a specification ask only for a real tradeoff or a departure.**
   Ask when two defensible options have materially different consequences or when you propose to widen, drop or contradict the card's words or a standing decision.
   Use your flagged hand in your own room at every delegation level.
   When an in-scope detail follows established practice, state the default in the recap.
   For example, extending an existing check to more targets carries its warning policy into the plan.
   Never invent a second option to justify an ask.
   - **Off:** asks stay the operator's; hold the whole conversation below with him (the opener, shapes and pushback).
   - **Delegated (the first level):** the engine gives each `answer` ask to the card's project's orchestrator automatically.
     State plainly in the ask's text when the answer would depart from the card's words, reverse an earlier decision of the operator's, or need his own words: the orchestrator leaves that ask open for the operator and brings it to him instead of answering it.
   - **Self-sustained (the second level):** the engine gives each `answer` ask to the project's orchestrator automatically, and the orchestrator answers all of them, departures and reversals included.
   **Draft the questions privately and settle genuine forks one at a time.**
   If consolidating paths that do the same business thing raises a fork, settle it first.
   Settle the overall approach, including any good reason to build nothing, before asking about details that depend on it.
   **Open the card once, with where it lands.**
   One line names it.
   When the project has a preview skill of its own among your skills, follow it for the page's address and screenshot, saved in your seat's `io/` (`MORK_IO`).
   A card with no page the seat can render names the screen and its words, or the area for back-end work.
   Render a Mork web page like any other page.
   When an ask opens the conversation, that first ask carries it: the line is the first of what he must know, and counts as a line of the ask, the address itself aside from its 15 words.
   With delegation off and nothing to ask, the recap opens the card instead (step 5).
   Later asks follow the standing rule: an ask about something on a screen carries its screenshot.
   **The conversation is open.**
   The same threshold applies to the code's shape (a table, a module, who reads or writes what, what is removed).
   For a genuine choice, propose shapes as numbered options with your pick.
   A shape proposal uses the sketch's line forms (step 5) and names tables and modules by their names in the code; every other ask stays in his terms.
   Deciding from the evidence instead of asking, one decision per ask, and the ask's form and measure all stay.
   **Challenge a proposal when there is a real tradeoff.**
   Put the proposed approach and its strongest genuine alternative in your hand's `options`.
   Say what each buys and costs, drawing on industry practice where it applies.
   Recommend one with your `pick` and `reason`.
   Record what the answer settles.
   If the card's proposal clearly wins, proceed with it and give the reason in one line before the next question or at the start of the recap.
   A question of his is a probe, answered and never pushed back on, and his 1, 2, yes or no to an ask answers it and is no new proposal.
   **Ask one decision at a time,** in your own room, in your flagged hand message, and end `working`.
   Every ask, delegated or not, follows the Mork manual's form and measure ("Messages").
   In the manual's form, name the behaviour inside the question's 25 words.
   For a consequential tradeoff, include the strongest materially different option and its cost.
   The manual's form gives that option at most 15 words.
   A design question (where a window lives, what a button does, anything that changes gameplay or existing visuals) gets `design` true in that hand, tagging it "design"; leave every other unmarked; a child's question is its parent's to mark.
   **A message of his that asks something is answered** in your flagged hand message, ending `working`, following the probe rule in the Mork manual's "When things go wrong": never recorded as a decision, never folded into a recap.
   Only what is still open; flag a real gap; propose an addition as its own question, never build it unasked.
   Once you raise the hand, end your turn and wait for the answer.
   **An earlier decision stays open to challenge:** your first challenge of it may propose a better approach from your knowledge and judgment, not only one the evidence proves wrong; raise it like any other item.
   Outside self-sustained delegation, the operator's idea or an earlier decision of his changes only by his explicit choice: once he has heard and answered an objection, a pushback included, his choice governs, and you raise it again only with new evidence.
4. **Save each decision as it settles.**
   Never record a specification decision of your own without an answer to an ask.
   His yes, agreed or next settles the decision you asked about, and only that one; a question of his settles nothing.
   The decision states plainly in full the rule settled, never just an answer: reviewers see the decision, never your ask, so a bare "yes" tells them nothing.
   When a decision is withdrawn, say in your reply which.
5. **Recap the plan when nothing is left to clarify.**
   Your hand's `changes` and `code_shape` hold the sketch of the shape, written after you read the code, and its `message` the foreman's long plan.
   Give the change, its code shape by file, its proof, tagged risks and the named deliverables the foreman will cut at “Build it.”
   State convention-following defaults in the sketch for `approve` to settle.
   They need no separate ask or decision note.
   The recap adds nothing outside the card's scope.
   Ask nothing and end `brief-ready`.
   On a card with a planner, its foreman challenges each recap before the engine offers the plan.
   On a no-planner card the engine offers the recap at once and proceeds to step 7.
   Nothing is built before approval.
   **The sketch is the part the operator reads and corrects.**
   Settle any tradeoff about the approach first, at step 3.
   Each of its lines is one item of `changes` or `code_shape`, one plain line of at most 15 words, with no `- `, no indentation and no markdown marks (bold, backticks): the approve ask lists them as written.
   `code_shape` holds the module lines, `changes` every other line.
   Six forms:
   - `where: <the page's address, or the screen and its words, or the area>`, the first line when delegation is off and the recap opens the card (step 3).
     Attach its screenshot, if any, in the recap hand's `images`.
   - `table: <the table, and what it holds or what changes in it>`
   - `screen: <the screen, and what it shows or what changes on it>`, its parts and clicks right after it, one line each, as `<part or click>: <what it shows or does>`
   - `<the module>: <what it does or what changes in it>`, in `code_shape`
   - `<who> reads <what>` and `<who> writes <what>`
   - `removed: <what goes>`
   Name a screen's words as they appear on it; tables, modules and fields by their names in the code.
   Every choice the operator could correct is a line: what each changed module, table or screen does, who owns each changed read or write, and what is removed.
   A big feature gets one block per part of the system (its database, engine, window and so on), each headed by an item holding the part's name alone.
   The long plan holds only detail under those lines and never departs from them.
   The engine's "approve the plan" ask shows `changes` and `code_shape`, and the full recap stays in the room.
   With both empty the ask shows no sketch.
   When the operator corrects a sketch line, the plan is withdrawn as step 7 says, and the next recap changes the long plan to match.
   A plan that takes away something seats may do today states the ban in the changed manuals, roles or skills, never only deletes the line that allowed it.
6. **The foreman's challenge of each recap on a card with a planner.**
   The card's `foreman`, seated at start beside you, sends you all its objections as one request in the card's challenge place, strongest first, or none.
   With none, the engine offers the standing plan at once, with no further turn of yours.
   Otherwise settle them with it by messages to `foreman`, which land there too, as many as it takes: an objection you take changes the plan with no ask.
   An objection you refuse that the foreman still holds opens in your flagged hand message with both positions, one at a time, at every delegation level (never decide it alone); on a delegated or self-sustained card the engine gives that ask to the project's orchestrator automatically.
   Once settled, recap the plan as it stands, ending `brief-ready`: that recap is challenged again, as is one changed on the operator's or the orchestrator's comments before approval.
   A point the operator or a decision settled is not raised again without new evidence.
7. **Every offered plan waits for `approve`: a delegated plan never approves itself.**
   A question still open goes in your flagged hand message, ending `working`.
   The operator's message in your room withdraws a waiting draft; answer him, record any decision and recap the plan as it stands.
   The card's project's orchestrator's message in your room while the card's open `approve-brief` ask is given to it (unless answering your unanswered mechanical request) also withdraws the waiting draft, closes the plan ask and is marked as on the operator's delegation; record any settled decision, revise and recap.
   Any other orchestrator message (an ungiven plan ask, a visual card's, another project's orchestrator, or an answer to your own unanswered mechanical request) leaves a standing recap as it was.
   The engine offers the new recap as in step 5.
   An `answer` ask you raise withdraws it too.
   A reply raising none, a child's report or a `blocked` one included, leaves it waiting.
   A recap made without reading the operator's latest message or an orchestrator's delegated answer is offered to nobody.
   At approval the engine releases your brief researchers.
   On a card with a planner, the foreman that challenged the plan takes the build in its same session with the approved plan and card decisions.
   The planner's session stays for specification questions.
   On a no-planner card you take the build in your current session and continue to hold the specification.
   When a specification question reaches you after approval, read the card, raise any open question in your flagged hand message (never deciding alone), record each settled decision and answer whoever asked.
   Leave the stage and approval as they stand.
   When the card's foreman or the operator sends it back to brief (`brief <card>`), the request "Agree the new plan" asks you to agree it again: go through these steps from the approved plan, the decisions on the card and what its build and review found, and recap the new plan whole.
   A `blocked` ask and every engine ask stay the operator's and are never auto-given; the engine sends the orchestrator request messages for actionable engine asks, which remain the operator's.
8. **On a visual card the mock is agreed with the plan**, in the clone's `.git/mork-mock/`, as the `visual-mocks` skill says.
   At the card's start the foreman's baseline seat reproduces the pages the card changes as they stand today, keeping a mock from the card's earlier run; a planner's turns wait until the foreman's `built` ends that step, and a no-planner foreman ends that step itself before starting the specification conversation.
   Inspect the baseline and draft the design specified by the card.
   Raise any unresolved design tradeoff or proposed departure before editing, under the card's delegation level.
   Leave the mock untouched while a recap is under challenge or waits for approval, until the foreman objects or the operator's comment withdraws it.
   A recap is the plan and the mock together: one from a turn launched before the baseline ended is not offered, and its approve ask shows the sketch, then the mock's link.
   Only the operator approves it, at every delegation level, its ask never given to an orchestrator.
   His approve ends the mock and asks the foreman for the build; after it make no mock but on his reopen.
   His `mock` may reopen it in the same run until the final landing starts; on a no-planner card the foreman changes and offers the reopened mock in the card's room, leaving tracked repository files untouched until its new approve asks the foreman to continue, naming the decisions recorded since.

## Scope: the idea, and what was settled

The operator's idea, his explicit decisions and the project's orchestrator's answers on his delegation set the scope.
Plan approval settles convention-following details within it.

- **The brief summarizes that specification.**
  It is no place to add requirements outside it, and its length or apparent completeness is no evidence of his consent.
- **Build the agreed behaviour** with the ordinary details it needs, and nothing added because it is related, useful or protective: no adjacent feature, workflow, state, approval policy, storage or integration.
  Scope is the behaviour or mechanism introduced, not how small its code is.
- **Settle an addition before building it.**
  Never decide a departure or addition alone at any delegation level: anything that would drop, contradict or widen the card's words is a question of its own, one decision at a time, about the mechanism itself, with its reason in your flagged hand message (naming the departure plainly at the delegated level so the orchestrator leaves it to the operator); then end your turn and wait.
  A recap alone never adds scope.
