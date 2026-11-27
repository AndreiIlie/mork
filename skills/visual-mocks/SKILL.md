---
name: visual-mocks
description: "Use on any visual card's mock (Mork web pages included), or to render a web page with the wall's headless Chromium: how the browser is launched, visual mode's baseline, mock folder and host, how a mock is agreed and reopened, and how a Mork web page's mock imports React and compiles its CSS."
---

# Visual mocks

## Headless Chromium

A headless Chromium is installed outside seat homes, in Mork's `browsers` folder, read-only in every wall and found by `PLAYWRIGHT_BROWSERS_PATH`: launch `$PLAYWRIGHT_BROWSERS_PATH/chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell` with `--no-sandbox --disable-gpu` (the wall is the sandbox) and a `--user-data-dir` in a folder you may write, and render a page you serve yourself (`--dump-dom`, or `--screenshot=<file>.png --window-size=1280,800 <url>`).
The wall also shows the system Chrome, including its target outside `/usr`; use a writable `--user-data-dir` for it too.
Give a library such as Puppeteer the installed binary as its `executablePath`, with `--no-sandbox`, never its own download.
A screenshot meant for your reply goes in your seat's `io/` folder (`MORK_IO`).
Render the baseline at the start of the visual brief when there is a page to reproduce.
Render a mock before each recap or offer of it when the request gives a link, and during review when the round requests it.
Otherwise, render when seeing the page settles something reading its code leaves open.
A project may say more on what a render loads in a preview skill of its own among your skills.

## Visual mode

On a visual card the mock is made at brief and approved with the plan.
Once the clone is made it opens at its baseline step: the engine asks the foreman to build the baseline and, on a card with a planner, asks the planner to agree the specification, its turns waiting; on a no-planner card, "Agree the specification" comes once the baseline ends.
The foreman names the pages the card changes and spawns its baseline seat, which reproduces each as it stands today in the clone's `.git/mork-mock/`, untracked and never committed; the foreman writes nothing there during the baseline step.
The foreman's `built` ends the step and releases the seat, and the folder is then the specification holder's (the planner's, or the foreman's on a no-planner card), writable to the planner at brief and on a reopen while a no-planner foreman leaves tracked repository files untouched; one writer at a time.
Its host reads the clone and writes only `.git/mork-mock-host/`, its log in `host.log` there.
When the request gives a link, the engine serves it there from the step's opening; no seat starts or stops its host.
When it gives none, nothing serves the mock: the specification holder raises that in its flagged hand message.
Follow the project's written rules for its look (`web/DESIGN.md` for Mork web pages), and keep any mock already in the folder, an earlier run's included: a baseline fills only the surfaces it lacks.
A mock sets no `<base>`; a project file's address is written in full, starting `/@fs/<the clone's path>/`.

The baseline seat renders each page it reproduced once at its path under the mock link before it reports.
Once the baseline step ends, the planner (or the foreman on a no-planner card) holds the visual brief in two steps.

1. **Read the card against the baseline**

   Render and inspect the existing baseline.
   When the card specifies the design, go straight to the first draft.
   Ask about direction, information hierarchy or concrete changes only for a real tradeoff or proposed departure.
   Attach the baseline screenshot to that design ask.
   Use an ASCII layout sketch in the conversation when it helps.
   Record decisions on the card as they settle.
   When a design ask is needed, wait for its answer before editing `.git/mork-mock/`.
   With delegation off, the operator answers the design ask in your room.
   On a delegated card, mark the ask as needing his own answer so the orchestrator brings it to him.
   On a self-sustained card, the orchestrator answers it.

2. **Make the first draft**

   Apply the specified design and any answered decisions together in the first draft.
   Show convention-following details in the recap with the mock for plan approval.
   Record decisions settled by answers as the mock is refined.

Before each recap, render the mock once from the link exactly as the request gives it, with nothing added.
Leave the served mock untouched while a recap is under challenge or waits for approve; edit it only once his comment withdrew the recap or the foreman's objection came, then recap again.
The link keeps showing what he was shown.
After the first draft, apply each agreed refinement in place in the turn it is recorded.
Put any new or alternative draft at a sub-path linked from it.
A recap is the plan and the mock together: on a card with a planner the foreman's challenge request names the link, or says nothing serves it, and the plan's approve ask shows the sketch, then the link.
That approve is the operator's alone at every delegation level, never given; it ends the mock and asks the foreman for the build.
No mock is made after it but on the operator's reopen.

Visual mode turned on at brief withdraws a waiting recap and opens the baseline step; turned off there, it ends the mock and releases the baseline seat; turned on after the plan's approve, no mock opens.

The operator's `mock <card>` reopens an approved mock in the same run until the card's final landing starts.
The card's whole submission is withdrawn and it returns to build.
Running turns finish and the planner (or the foreman on a no-planner card, editing no tracked repository files with `built` refused) mocks again at once.
It offers the ready mock with its link, asking nothing, ending a clear `brief-ready`: that opens a `mock` ask in its room with an approve button, the operator's alone.
Plain words never approve it.
The offer and its approve ask close whenever a turn of the mock holder starts.
Its next clear `brief-ready` raises a new ask and notifies the operator again, unless a message to it was left unread: that message already owes it another turn.
Approve is refused while the offer is down.
Every reply then names the mock's link first, when the request gave one.
On a card with a planner, no new turn of the foreman starts until the new approve; the approve asks the foreman to continue the build, naming the decisions recorded since the reopen.
Before a card's clone is removed, its mock is kept in Mork's home.
The card's next run gets it back in its new clone, for its baseline seat to keep and its specification holder to carry on from at brief; its link may change.

A project may add its own mock rules in a preview skill of its own among your skills.

## Mork web pages

A Mork web page mock needs `web/node_modules` in the clone, made by the project's `install`; when it is missing, raise it in your flagged hand message.
Follow `web/DESIGN.md` for tokens, type scale and where colour is allowed.
To show a page as it stands, import its workspace components (`Sidebar`, `Board`, `RaisedHands`, `CardRoom`) by their `/@fs/` address and mount them with fixture props written in the mock, the props their callers in `web/src` pass (`App.tsx` for most, `Board.tsx` for `RaisedHands`; `CardRoom` needs a `views` map holding its card's `PlaceView` beside the `Screen`, or it shows only its loading screen), never `/@fs/<the clone's path>/web/src/main.tsx`, which dials the bridge and shows "Not connected" on the mock host.
Give the mock's `index.html` the `<body>` classes of `web/index.html`, because `web/src/index.css` does not set `font-sans` on `body`.
Write mock `.tsx` files as `web/src`'s are, importing `react` and `react-dom/client` by their bare names: the host resolves them to the clone's `web/node_modules`, so the mock and the workspace components share one React.
Add `mock.css` in the mock folder with `@import "../../web/src/index.css";` followed by `@source ".";`, and link it from `index.html`: the host compiles it with the workspace's Tailwind on every change.
A `postcss.config.*` of the mock's own, in the mock folder, replaces that Tailwind, and the host restarts whenever one is created, changed or removed there.
