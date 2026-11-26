---
name: messages
description: "Use when writing a message to another agent or an orchestrator, or raising a hand to the operator: the three kinds and how an exchange ends, and Mork ctl say form."
---

# Messages

The rule is the Mork manual's ("Messages"): a message is words, it wakes the agent it reaches unless handed to its running turn, and it does nothing else.

**Kinds.**
`request` asks and wants a response; `response` answers one and wants nothing back; `update` informs and wants nothing back.
A response or an update is never answered: an exchange ends with the response to its request, no thanks or agreement sent back.
A follow-up is a new request.
Questions nest:

- "Did you eat today?": a request.
- "What day is it?": a request, before the answer.
- "Wednesday.": a response, to the second request.
- "I didn't eat today.": a response, to the first.

An ask to the operator about something on a screen carries a screenshot of it where you can render it.
For a web page, use the wall's headless Chromium, as the `visual-mocks` skill says.
When the project has a preview skill of its own among your skills, follow its rendering rules.
For an ask to the operator about Mork's native window, use the words shown on screen.
To show the operator a layout or a mechanism, or to hand an agent a picture, put a screenshot or a diagram in your seat's `io/` (`MORK_IO`) and list its full path in the `images` of your hand (shown beside the hand's message) or of a message (carried into its recipient's turn).
The engine copies each file, so later edits don't change it.
`mork ctl say <place> --kind <kind> <text>` gives it too.
The news says each message with its kind's verb: `ex-550/foreman asks researcher-1: …`, `ex-550/researcher-1 answers foreman: …`, `ex-550/researcher-1 notes to foreman: …`.

A parent seat's message to its child lands in the child's DM.
A child's message to its parent lands in its parent's room.
A researcher's parent is the seat that spawned it.
A full card with a planner has two rooms, each with one seat: the build room, `<card>`, the foreman's, and the spec room, `<card>/spec`, the planner's; an unaddressed message in either reaches its seat at every stage.
The foreman and the planner write to each other by name, `planner` and `foreman`, at every stage: their messages, and their unnamed words said in the other's room, land in the card's challenge place, `<card>/challenge`, which the window shows nowhere, and their steps sending them show without the text, as does a `say` whose place or recipient its command line doesn't show.
The planner answers the foreman's DM by `foreman/dm`, which reaches that seat alone.
The operator's or an orchestrator's word naming the other room's seat is refused: `planner` in the build room, `foreman` in the spec room.
Opening the planner's ask opens the spec room for the reply.
A card started with no planner has one room: an unaddressed message there reaches its foreman at every stage, `planner` is refused, and the foreman holds the specification itself, raising open questions in its flagged hand message.
Never send the orchestrator a specification question as a message (`say` or a reply's `messages`): every specification question goes in your flagged hand message, which the engine automatically gives to the project's orchestrator on a delegated or self-sustained card.
Direct request messages to `orchestrator` are for mechanical needs only.

Your session's start says exactly whom you reach from where you are; a change comes as a Mork announcement.
Nothing counts an exchange's turns or stops it: loops are discouraged by what the kinds mean.

**Writing to the operator,** only in your flagged hand message, then ending your turn to wait for his answer:

For a decision ask, the Mork manual's measure wins over every line below: cut what does not fit, never move it to another ask.
An answer to his question is one or two plain sentences, detail only when he asks; a report he asked for is complete; a failure is in full.
A planner or foreman whose system prompt carries the operator's output style writes by that style where it differs from this form and these lines.

- Take his question at face value.
  Answer it first in your flagged hand message, with yes or no and what the code says.
  An angry question still wants an answer, not a build or a rewrite: his questions test the quality of your work.
- Use plain language and one short question.
  When he asks why something happens, give the cause.
  Include the formula or code location when he asks for it.
- In a decision ask, the who, where or when he needs to pick go in the lines before the question, not inside it; the question names the behaviour in its 25 words, and the reason stays in the hand's `reason`, at most 15 words.
  Never open by acknowledging the previous decision, put nothing of the kind between the headline and the question, and never repeat the headline in the body.
  An ask he can't read wants asking again in plain words, never deciding for him.
- Propose the minimal version of a mechanism first: no derived statistics, no verification machinery he didn't ask for.
- A proposal to change content (titles, texts, numbers) names the exact items and their old and new values within the decision ask's measure: a rule or a criterion alone doesn't show him the result.
  A report with no decision keeps the old and new values side by side in a short table.
- Write everything needed to answer in the flagged message, in his terms, what the person sees and does, never code names, except a question about the code's shape, which may name tables and modules as the sketch does.
  A decision ask may carry a tiny worked example (bill 1000 = BL1 600 + BL2 400) or a rule's formula and one worked number in a line before the question; otherwise cut them.
  A question about a state may use screenshots labelled 1 and 2, your pick after the options.
  A report he asked for may keep its full examples and formulas.
- Ask what his words left open, or challenge what they settled with your reason, never propose back what he already told you.
  A specification question stays with the work his words describe and may offer a code shape for it.
  An objection he has heard and answered stays answered unless you bring new evidence.
  Check he knows a behaviour exists before proposing a tweak to it: a surprising one is news to report.
  A list relayed as his may hold items he never said: settle it item by item.

The options go in the hand's `options`, which the engine numbers; the pick's number in `pick`, its why in `reason`.

Good:

```text
ask: true
message: Older bills carry the previous tenant's name.
     Should a tenant see the bills from before their contract started?
options: No: the list starts on the contract's first day.
         Yes: older bills show too, marked "before your contract".
pick: 1
reason: the previous tenant's name is personal.
```

Bad, the same question.
Message with `ask` true: "The card already says a tenant sees only their own apartment's bills, and the list is built per apartment (`BillList`).
Should older bills show, and if they do, should the total count them?
My pick is no for both.
After this I will ask about the export and the email."

What is wrong: it restates what the card settles; it holds two decisions; it trails the questions still to come after the ask, where each ask should stand alone; it names code; it offers no options.

Bad, one decision, the order and the numbering right, but too long.

```text
ask: true
message: Older bills show the previous tenant's name alongside the apartment and billing period, so tenants may learn who lived there before their own contract started.
     Should tenants who rent the apartment today be able to see those older bills with the previous tenant's name still visible alongside the other details shown on them, or not?
options: No. Show older bills with the previous tenant's name hidden and keep the apartment and billing period visible.
         Yes. Show older bills with the previous tenant's name visible alongside the apartment and billing period, as they appear today.
pick: 1
reason: the previous tenant's name is personal information the current tenant doesn't need to understand what the apartment was charged.
```

The question exceeds 25 words.
The background line, each option, and the reason exceed 15 words.
Cut the excess and keep the same decision and options.

Cut version:

```text
ask: true
message: Older bills show the previous tenant's name.
     Should the current tenant see the previous tenant's name on older bills?
options: No: hide the previous tenant's name.
         Yes: show the previous tenant's name.
pick: 1
reason: tenants understand past charges without it.
```
