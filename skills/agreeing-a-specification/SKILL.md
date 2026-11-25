---
name: agreeing-a-specification
description: "Use when you hold a card's specification and must settle it before anything is built: a full card's start. The method, delegation levels, and how a decision is saved on the card."
---

# Agreeing a specification

The planner settles a full card's specification at brief and on named requests after approval.
On a full card started with no planner (`start --no-planner`), its foreman holds the specification at brief and after approval.
Before approval that foreman writes, edits and commits nothing, except `.git/factory-mock/` on a visual card once its baseline step ends.

1. **Read the card first** (`factory ctl card.show <card>`) and what it names.
   **Scout the rest.**
   Give researcher seats the other questions the plan depends on: the code and data's facts, what to reuse and the house style, outside documentation, best practices, and whether several paths do the same business thing.
   How many and which questions are yours, with no fixed roster, and you may skip scouting when the card names the place already.
   The `delegating` skill has how to spawn and task them.
   Challenge the card with your knowledge and the evidence the scouts bring back.
   Propose whatever you would change, from what you know as freely as from what the scouts looked up.
   A proposal needs no cited source: Andrei filters the weak ones.
   Establish what the evidence establishes; never ask Andrei for it.
   Judge code questions by these three tests: 1. Modules built so that others sit on top of them: reusable, scalable.
   2. The simple approach (the hardest of the three to judge).
   3. A tiny feature that costs a lot of RAM or CPU is a candidate to drop.
   The bar chooses between solutions and adds no scope: the top rule still bounds what is built.
2. **Build on the card's words.**
   Its title and description are the issue and Andrei's words, whoever typed them; they and his explicit decisions, plus decisions settled on his delegation by the project's orchestrator answering given asks, are authoritative.
   Build on them and challenge an earlier decision by raising a question in your hand's `ask` (never deciding a change alone).
   A solution the text names is his idea, open to that same challenge.
   A note that is not a decision stays a proposal.
3. **Never decide a specification question alone: raise every open question in your hand's `ask`, at brief and after it.**
   The set of questions you ask is the same at every delegation level: establish from evidence what evidence establishes, and ask the rest in your hand's `ask`, in your own room (the spec room on a card with a planner), in the same form.
   - **Off:** asks stay Andrei's; hold the whole conversation below with him (the opener, shapes and pushback).
   - **Delegated (the first level):** the engine gives each `answer` ask to the card's project's orchestrator automatically.
     State plainly in the ask's text when the answer would depart from the card's words, reverse an earlier decision of Andrei's, or need his own words: the orchestrator leaves that ask open for Andrei and brings it to him instead of answering it.
   - **Self-sustained (the second level):** the engine gives each `answer` ask to the project's orchestrator automatically, and the orchestrator answers all of them, departures and reversals included.
   Never send a specification question to the orchestrator as a direct message (`say` or a reply's `messages`): direct request messages to the orchestrator are for mechanical needs only (a file, a path or a count outside your wall).
   **Draft every item privately, then settle them one at a time,** first, when step 1 found the code doing the same business thing in more than one way, whether to fold them into one, before every other item.
   Then whole-approach changes (a different mechanism, a simpler design, a reason not to build it), details after: an early detail settled is wasted if the approach under it changes.
   **Open the card once, with where it lands.**
   One line names it.
   When the project has a preview skill of its own among your skills, follow it for the page's address and screenshot, saved in your seat's `io/` (`FACTORY_IO`).
   A card with no page a seat can render (the factory's native window, back-end work) names the screen and the words shown on it, or the area for back-end work, with no picture; a Factory web page is rendered like any page.
   When an ask opens the conversation, that first ask carries it: the line is the first of what he must know, and counts as a line of the ask, the address itself aside from its 15 words.
   With delegation off and nothing to ask, the recap opens the card instead (step 5).
   Later asks follow the standing rule: an ask about something on a screen carries its screenshot.
   **The conversation is open.**
   Your questions are not only business questions: put the code's shape to him too (a table, a module, who reads or writes what, what is removed), and with any question you may propose shapes of your own, as numbered options with your pick.
   A shape proposal uses the sketch's line forms (step 5) and names tables and modules by their names in the code; every other ask stays in his terms.
   Deciding from the evidence instead of asking, one decision per ask, and the ask's form and measure all stay.
   **Push back once on every proposal of his** before recording it, a mechanism, a shape, a behaviour or a wording, never a bare agreement.
   Argue from industry standard; where none applies (two valid button labels), from the strongest alternative's concrete consequences on this card.
   When you would choose the alternative, put both in your hand's `options` and your pick and its reason in `pick` and `reason`.
   Record what is decided once answered.
   If his proposal follows the standard or is the better choice, record it and name the strongest alternative and why it loses in one line.
   Put the one-line pushback before the next question, after the opener.
   If the reply asks nothing, put it in your hand's `message`, its first line if you're recapping.
   A question of his is a probe, answered and never pushed back on, and his 1, 2, yes or no to an ask answers it and is no new proposal.
   **Ask one decision at a time,** in your own room, in your hand's `ask`, and end `working`.
   Every ask, delegated or not, follows the Factory manual's form and measure ("Messages").
   If your system prompt carries Andrei's output style, it takes precedence wherever it differs from that form or the formatting rules here.
   An answer to his question is one or two plain sentences, detail only when he asks; a report he asked for is complete.
   In the manual's form, name the behaviour inside the question's 25 words.
   For a consequential tradeoff, include the strongest materially different option and its cost.
   The manual's form gives that option at most 15 words.
   A design question (where a window lives, what a button does, anything that changes gameplay or existing visuals) gets `design` true in that hand, tagging it "design"; leave every other unmarked; a child's question is its parent's to mark.
   **A message of his that asks something is answered** in your hand's `ask`, ending `working`, following the probe rule in the Factory manual's "When things go wrong": never recorded as a decision, never folded into a recap.
   Only what is still open; flag a real gap; propose an addition as its own question, never build it unasked.
   Once you raise the hand, end your turn and wait for the answer.
   **An earlier decision stays open to challenge:** your first challenge of it may propose a better approach from your knowledge and judgment, not only one the evidence proves wrong; raise it like any other item.
   Outside self-sustained delegation, Andrei's idea or an earlier decision of his changes only by his explicit choice: once he has heard and answered an objection, a pushback included, his choice governs, and you raise it again only with new evidence.
4. **Save each decision as it settles.**
   Never record a specification decision of your own without an answer to an ask.
   His yes, agreed or next settles the decision you asked about, and only that one; a question of his settles nothing.
   Record it on the card at once with `factory ctl card.change <card> --decision "<the decision, stated plainly in full>"`: marked notes are part of the specification its reviewers see.
   The decision states plainly in full the rule settled, never just an answer: reviewers see the decision, never your ask, so a bare "yes" tells them nothing.
   An orchestrator's answer that the engine marks as on Andrei's delegation settles only the ask given to it; name that ask when recording its decision, and on a self-sustained departure name what it departs from.
   When a decision is withdrawn, say in your reply which.
5. **Recap the plan when nothing is left to clarify.**
   Your hand's `changes` and `code_shape` hold the sketch of the shape, written after you read the code, and its `message` the foreman's long plan.
   Give the change, its code shape by file, its proof, tagged risks and the named deliverables the foreman will cut at “Build it.”
   The recap adds nothing outside the card's specification.
   Ask nothing and end `brief-ready`.
   On a card with a planner, its foreman challenges each recap before the engine offers the plan.
   On a no-planner card the engine offers the recap at once and proceeds to step 7.
   Nothing is built before approval.
   **The sketch is the part Andrei reads and corrects,** for an approach already settled: push back on the task itself first, at step 3.
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
   Every choice Andrei could correct is a line: what each changed module, table or screen does, who owns each changed read or write, and what is removed.
   A big feature gets one block per part of the system (its database, engine, window and so on), each headed by an item holding the part's name alone.
   The long plan holds only detail under those lines and never departs from them.
   The engine's "approve the plan" ask shows `changes` and `code_shape`, and the full recap stays in the room.
   With both empty the ask shows no sketch.
   A pushback line due at the recap (step 3) is the first line of the hand's `message`, outside the sketch.
   When Andrei corrects a sketch line, the plan is withdrawn as step 7 says, and the next recap changes the long plan to match.
   A plan that takes away something seats may do today states the ban in the changed manuals, roles or skills, never only deletes the line that allowed it.
6. **The foreman's challenge of each recap on a card with a planner.**
   The card's `foreman`, seated at start beside you, sends you all its objections as one request in the card's challenge place, strongest first, or none.
   With none, the engine offers the standing plan at once, with no further turn of yours.
   Otherwise settle them with it by messages to `foreman`, which land there too, as many as it takes: an objection you take changes the plan with no ask.
   An objection you refuse that the foreman still holds opens in your hand's `ask` with both positions, one at a time, at every delegation level (never decide it alone); on a delegated or self-sustained card the engine gives that ask to the project's orchestrator automatically.
   Once settled, recap the plan as it stands, ending `brief-ready`: that recap is challenged again, as is one changed on Andrei's or the orchestrator's comments before approval.
   A point Andrei or a decision settled is not raised again without new evidence.
7. **Every offered plan waits for `approve`: a delegated plan never approves itself.**
   Once offered, its `approve-brief` ask opens and, on a delegated non-visual card, the engine gives that ask to the card's project's orchestrator automatically without a desktop notice.
   Turning `delegate` on while a plan waits gives the open ask without approving the plan.
   A visual card's plan, its mock included, waits for Andrei's own approve at every level, its ask never given.
   Every plan waits for the button or `approve` command from Andrei, or from its project's orchestrator while its open `approve-brief` ask is given to it.
   Andrei may still approve any given plan himself.
   Words in the room never approve.
   A question still open goes in your hand's `ask`, ending `working`.
   Andrei's message in your room withdraws a waiting draft; answer him, record any decision and recap the plan as it stands.
   The card's project's orchestrator's message in your room while the card's open `approve-brief` ask is given to it (unless answering your unanswered mechanical request) also withdraws the waiting draft, closes the plan ask and is marked as on Andrei's delegation; record any settled decision, revise and recap.
   Any other orchestrator message (an ungiven plan ask, a visual card's, another project's orchestrator, or an answer to your own unanswered mechanical request) leaves a standing recap as it was.
   The engine offers the new recap as in step 5.
   An `answer` ask you raise withdraws it too.
   A reply raising none, a child's report or a `blocked` one included, leaves it waiting.
   A recap made without reading Andrei's latest message or an orchestrator's delegated answer is offered to nobody.
   At approval the engine releases your brief researchers.
   On a card with a planner, the foreman that challenged the plan takes the build in its same session with the approved plan and card decisions.
   The planner's session stays for specification questions.
   On a no-planner card you take the build in your current session and continue to hold the specification.
   When a specification question reaches you after approval, read the card, raise any open question in your hand's `ask` (never deciding alone), record each settled decision and answer whoever asked.
   Leave the stage and approval as they stand.
   When the card's foreman or Andrei sends it back to brief (`brief <card>`), the request "Agree the new plan" asks you to agree it again: go through these steps from the approved plan, the decisions on the card and what its build and review found, and recap the new plan whole.
   Only a recap from a turn that read that request counts; it is challenged and offered as at the first brief, and its approve continues the build to the new plan.
   A `blocked` ask and every engine ask stay Andrei's and are never auto-given; the engine sends the orchestrator request messages for actionable engine asks, which remain Andrei's.
8. **On a visual card the mock is agreed with the plan**, in the clone's `.git/factory-mock/`, as the `visual-mocks` skill says.
   At the card's start the foreman's baseline seat reproduces the pages the card changes as they stand today, keeping a mock from the card's earlier run; a planner's turns wait until the foreman's `built` ends that step, and a no-planner foreman ends that step itself before starting the specification conversation.
   Then change the baseline with Andrei through the conversation, as you settle the plan (leaving tracked repository files untouched before approval): record each change as a decision on the card, render the mock from its link before each recap, and leave it untouched while a recap is under challenge or waits for approve, editing it only once his comment withdrew the recap or the foreman's objection came.
   A recap is the plan and the mock together: one from a turn launched before the baseline ended is not offered, and its approve ask shows the sketch, then the mock's link.
   Only Andrei approves it, at every delegation level, its ask never given to an orchestrator.
   His approve ends the mock and asks the foreman for the build; after it make no mock but on his reopen.
   His `mock` may reopen it in the same run until the final landing starts; on a no-planner card the foreman changes and offers the reopened mock in the card's room, leaving tracked repository files untouched until its new approve asks the foreman to continue, naming the decisions recorded since.

## Scope: the idea, and what was settled

A card's specification is Andrei's initial idea and his explicit decisions, plus decisions settled on his delegation by the card's project's orchestrator answering given asks, as the Factory manual's top rule says.

- **The brief summarizes that specification.**
  It is no place to add requirements outside it, and its length or apparent completeness is no evidence of his consent.
- **Build the agreed behaviour** with the ordinary details it needs, and nothing added because it is related, useful or protective: no adjacent feature, workflow, state, approval policy, storage or integration.
  Scope is the behaviour or mechanism introduced, not how small its code is.
- **Settle an addition before building it.**
  Never decide a departure or addition alone at any delegation level: anything that would drop, contradict or widen the card's words is a question of its own, one decision at a time, about the mechanism itself, with its reason in your hand's `ask` (naming the departure plainly at the delegated level so the orchestrator leaves it to Andrei); then end your turn and wait.
  A recap alone never adds scope.
