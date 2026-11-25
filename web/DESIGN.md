# Web Design System

Restrained developer-tool visual system for the web UI, built on Cold Geist dark tokens (Vercel Geist neutral greys), Geist Sans and Geist Mono typography, monochrome neutral surfaces, crisp hairline borders, sharp `0px` corners, flat rows, high-contrast primary actions, and colour reserved strictly for status dots, the Raised Hand card header label, errors, cache-expiry countdowns, and quota progress pace.

Dark only (`color-scheme: dark`). No light theme.

All tokens are defined in `web/src/index.css` inside `@theme`, where Tailwind's default colour palette (`--color-*`), radii (`--radius-*`), and text scale (`--text-*`) are reset to `initial` so only the tokens below exist. Global `html, body` and `:focus-visible` rules live in `@layer base` so utility overrides (such as `focus-visible:outline-none` on the fused composer textarea) work cleanly while keeping `:focus-visible` outlines everywhere else.

## Fonts

Bundled locally through `@fontsource-variable/geist` and `@fontsource-variable/geist-mono` (no CDN requests):

| Token | Value | Use |
|---|---|---|
| `--font-sans` | `"Geist Variable", system-ui, sans-serif` | Headings, UI labels, body copy, buttons |
| `--font-mono` | `"Geist Mono Variable", ui-monospace, monospace` | Card ids, build hashes, counts, stages, statuses, durations, countdowns, quota percentages, chart axes |

Always pair numeric counters and durations with `tabular-nums`.

## Colour and Spacing Tokens

Cold Geist dark values (locked to `docs/ui-reference-mock.html`, keeping `gray-700` at 56% for 4.5:1 muted-label contrast on `background-100`):

| Token | Value | Use |
|---|---|---|
| `background-100` | `hsl(0 0% 4%)` | Primary canvas, sidebar, secondary button fill, primary button text |
| `background-200` | `hsl(0 0% 6%)` | Composer strip and subtle recessed sub-surfaces |
| `gray-100` | `hsl(0 0% 9%)` | Row hover fill, button hover fill, Raised Hand card fill, composer focus fill, Andrei's stream message block |
| `gray-200` | `hsl(0 0% 12%)` | Active nav/card/row fill, selected segment, progress bar track, highlighted recommended choice button (`border-gray-700 bg-gray-200`) |
| `gray-300` | `hsl(0 0% 16%)` | Subtle interactive surface step (reserved in Geist scale) |
| `gray-400` | `hsl(0 0% 18%)` | All 1px hairline borders, dividers, segmented control frame, Andrei's stream message block border, scrollbar thumb |
| `gray-500` | `hsl(0 0% 27%)` | Idle/disconnected status dot, bordered button hover border, Raised Hand card border, scrollbar thumb hover |
| `gray-600` | `hsl(0 0% 53%)` | Muted neutral step (reserved in Geist scale) |
| `gray-700` | `hsl(0 0% 56%)` | Tertiary text (idle state, timestamps, sidebar build/stage/group/delegated text, backlog statuses, chart axes, helper notes) |
| `gray-900` | `hsl(0 0% 63%)` | Secondary text (section counts, metadata, roles, subtitles, neutral progress fill, ghost button label, text link) |
| `gray-950` | `hsl(0 0% 78%)` | Primary button hover fill and border |
| `gray-1000` | `hsl(0 0% 93%)` | Primary text, card ids, headings, focus outline, primary button background, strong progress fill, elapsed tick |
| `gray-alpha-400` | `#ffffff24` | Text selection highlight (`selection:bg-gray-alpha-400`) |
| `red-700` | `hsl(358 75% 59%)` | Red status dot (error / red hand) and red `Raised Hand` header label |
| `red-900` | `hsl(358 100% 69%)` | Error text (inline command refusals) |
| `amber-700` | `hsl(39 100% 57%)` | Yellow/amber status dot (waiting hand, reconnecting/reloading banner, connecting link), amber `Raised Hand` header label, over-pace quota bar fill and percent text |
| `amber-900` | `hsl(39 90% 50%)` | Cache-expiry countdown text (`cold in Xm`) |
| `green-700` | `hsl(131 41% 46%)` | Green status dot (connected link, actively working seat or card) and on-pace quota bar fill |

Two spacing tokens are declared in `@theme`:

| Token | Value | Use |
|---|---|---|
| `--spacing-gutter` (`p-gutter`, `px-gutter`) | `24px` | Outer gutter for pages, page headers, pane headers, full-bleed pane rows, and composer |
| `--spacing-edge` (`p-edge`, `px-edge`) | `16px` | Inner edge padding for the left rail, bordered list rows, and cards |

Row vertical padding (`py-3`, `12px`) and gap (`gap-3`, `12px`) use Tailwind's standard scale.

### Where Colour Is Allowed

Colour is permitted in five places only:

1. **Status dots (`Dot` / `StatusDot`) and favicon badge**: `red-700`, `amber-700`, `green-700`, and neutral `gray-500`. Working cards pulse gently with `.animate-pulse-soft` (disabled under `prefers-reduced-motion`). In the Card Room, yellow dots are omitted from stream message hands and seat buttons/headers (`asking` renders `gray-500`); the Overview's current-stage row carries one status dot (red, amber, pulsing green, or grey). The browser tab favicon mirrors `FactoryMark` (`background-100` / `gray-1000`) and adds a top-right circular `red-700` or `amber-700` badge dot when hands wait on Andrei.
2. **Raised Hand header label (`HandCard`)**: compact square `h-5 px-2` label in the `HandCard` header (`bg-amber-700` for yellow asks, `bg-red-700` for red asks, `text-background-100`), with the rest of the card strictly monochrome (`bg-gray-100`, `border border-gray-500`).
3. **Error text**: `red-900` for inline command refusals and error messages.
4. **Cache-expiry countdowns (`Cold`)**: `amber-900` monospace text (`cold in Xm`), with no pill or row tint.
5. **Quota progress pace (`ProgressBar` and over-pace percent)**: `green-700` fill while projected window end is within 100%, `amber-700` fill and `amber-700` percent text when projected past 100% or reached.

Everything else stays strictly monochrome.

## Type Scale

Geist's named scale, exposed as `text-<name>` utilities (no intermediate sizes are added; every text uses the nearest named token):

| Token | Size / Line height | Weight | Letter spacing | Use |
|---|---|---|---|---|
| `heading-24` | `24px / 32px` | `600` | `-0.96px` | Page titles (`Board`, `Usage`, card title on `/cards/<id>`) |
| `heading-16` | `16px / 24px` | `600` | `-0.32px` | Centered state headings (`Not connected`, `Connecting…`) |
| `heading-14` | `14px / 20px` | `600` | `-0.28px` | Section headings (`Raised Hands`, `Active Cards`, `Merge Queue`, `Project Backlogs`, `Daily Spend & Landed Cards`, `Account Quotas`, `Burn by Role`), brand title |
| `label-14` | `14px / 20px` | `400` | `0` | Row titles, primary nav items, root text size |
| `label-13` | `13px / 16px` | `400` | `0` | Card ids, metadata, working seats, project names, segmented controls, `Button` md, `TextLink`, banners, pipeline steps |
| `label-12` | `12px / 16px` | `400` | `0` | Counts, timestamps, stages, statuses, countdowns, sidebar items, `Button` sm, quotas, chart axes, helper notes |
| `copy-14` | `14px / 20px` | `400` | `0` | Raised hand body words, composer input, placeholder/waiting descriptions |
| `copy-13` | `13px / 18px` | `400` | `0` | Inline error text under a hand |

## Radius, Borders, Spacing Rules, and Focus

- **Radius**: `0px` everywhere except circular `8px` status dots (`rounded-full` inside `web/src/ui/Dot.tsx`).
- **Borders**: `1px solid var(--color-gray-400)` (`border-gray-400` / `divide-gray-400`) for structural hairlines: sidebar edge, header/footer rules, banners, row lists, segmented controls, and secondary buttons; `border-gray-500` for the Raised Hand card frame and bordered button hover.
- **Outer gutter and inner edge**: One outer gutter (`--spacing-gutter`, `24px`) for pages, page headers, split panes, pane rows, and composer; one inner edge (`--spacing-edge`, `16px`) for the left rail, bordered list rows, and cards.
- **Rows own their padding**: Bordered containers holding full-bleed rows (`List`, `PaneList`, rail groups) have **zero vertical padding** (`py-0`) so active and hovered rows sit flush against hairlines with no dark gap.
- **Indents**: Nested rows add `+24px` (`indent={1}`) or `+48px` (`indent={2}`) on the left only, keeping the right edge constant.
- **No negative margins**: Never use negative margin utilities (`-mx-*`, `-mt-*`, etc.) to pull lists or sections out of padded containers.
- **Charts**: Every axis and value label is rendered as HTML around the plot (`ChartLayout`), never as `<text>` inside a `preserveAspectRatio="none"` SVG.
- **Focus**: Global `:focus-visible` ring in `@layer base` is `2px solid var(--color-gray-1000)` with `outline-offset: 2px`, avoiding the browser's default blue focus ring while letting utilities override it where needed.
- **Checks gate enforcement**: `cargo xtask check` and `cargo xtask checks` scan `web/src/**/*.{ts,tsx,css}` outside `web/src/ui/` and refuse, with located findings, any `rounded` / `rounded-*` utility and any negative-margin utility (`-m-`, `-mx-`, `-my-`, `-mt-`, `-mr-`, `-mb-`, `-ml-`, `-ms-`, `-me-`), with or without variant prefixes.

## Primitives (`web/src/ui/`)

Shared sharp-layout primitives live in `web/src/ui/`. **No primitive accepts a `className` prop.** Variation is expressed through typed props only so pages compose primitives instead of overriding styles, introducing ad-hoc offsets, or duplicating row/list class constants.

| File | Exports | Typed props and contract |
|---|---|---|
| `web/src/ui/Button.tsx` | `Button` | `tier` (`"primary" \| "secondary" \| "ghost"`), `size` (`"md"` 30px / `px-3` / `label-13`, `"sm"` 26px / `px-[10px]` / `label-12`), `pressed` (active `border-gray-1000 bg-gray-200` on `secondary`, setting `aria-pressed`), `square` (width equal to height, `26px` sm / `30px` md, no side padding, for an icon alone), `type`, `disabled`, `onClick`, `title`, `aria-*`. Centred label, equal side padding; primary hover uses `gray-950`. |
| `web/src/ui/TextLink.tsx` | `TextLink` | `to` (renders `PageLink`), or `onClick` / `disabled` / `title` (renders button). Plain `label-13` `gray-900`, hover `gray-1000` with underline (`3px` offset), no box or padding. |
| `web/src/ui/Segmented.tsx` | `Segmented` | `options`, `value`, `onChange`, optional `label`. Bordered `divide-x`, `28px` `label-13` segments; active segment uses `bg-gray-200 font-medium text-gray-1000`. |
| `web/src/ui/List.tsx` | `List`, `PaneList`, `Row` | `List` (bordered, zero padding) and `PaneList` (full-bleed edge-to-edge hairlines, zero padding). `Row` takes `indent` (`0 \| 1 \| 2`), `pane` (`boolean`), `onClick`, `expanded` (`aria-expanded`), `to` (`PageLink`), `actions` (sibling slot outside the trigger so buttons never nest inside `<button>` or `<a>`), and `active` (`bg-gray-200`). Interactive rows highlight `hover:bg-gray-100`; plain rows have no hover. |
| `web/src/ui/ProgressBar.tsx` | `ProgressBar` | `percent` (0–100), `tone` (`"neutral"` `gray-900`, `"ok"` `green-700`, `"hot"` `amber-700`, `"strong"` `gray-1000`), `height` (`2 \| 4 \| 6`), optional `tick` (0–100, 1px `gray-1000` line extending 2px above and below the track). |
| `web/src/ui/ChartLayout.tsx` | `ChartLayout` | `yLabels`, `xLabels` (plain strings or `{ full, short }` pairs switching to `short` below `lg`), `height` (`72 \| 156`, default `156`), `headroom` (pads both the y-labels and plot area by `16px` at top so bar value labels fit above the bar ceiling), `xAlign` (`"between" \| "columns"` to centre labels under equal columns), `children` (the stretched plot slot). Renders HTML `font-mono label-12 tabular-nums` labels around a bordered plot area. |
| `web/src/ui/Kpi.tsx` | `KpiStrip` | `items` (`label`, `value`, `sub`). One bordered horizontal row of four equal KPI cells (two by two below `lg`), the one allowed tile row, heading Board and Usage. |
| `web/src/ui/Section.tsx` | `SectionHead` | `title`, optional `note` (mono count/note), optional `right` slot. Shared section heading line above lists. |
| `web/src/ui/SplitPane.tsx` | `SplitPane` | `left`, `right`. Two `basis-1/2` panes separated by `border-l border-gray-400`, each scrolling independently. |
| `web/src/ui/PinnedScroll.tsx` | `PinnedScroll` | `children`, optional `scrollRef` (receives the scroll box, for a caller restoring its place after prepending rows), optional `keepRef` (set by that caller once it restored the place, so the next resize keeps it instead of re-pinning). A `relative flex-1` column whose `overflow-y-auto` box stays at its bottom while the reader is within `24px` of it, re-pinning on every resize of the box or its content (late images above or below the view, new rows, fonts, a neighbouring dock). Scrolling up, or a resize after the reader's own click inside it (an unfold), keeps the reader's place and unpins unless the pane is still at its bottom; an unpinned pane pins again once a scroll or resize leaves it at its bottom. While scrolled up it shows a centred `Button` (`secondary`, `sm`, `square`) with `ArrowDownIcon`, `aria-label` `Scroll to bottom`, `12px` above the pane's bottom edge; a click jumps to the bottom and pins again. |
| `web/src/ui/Composer.tsx` | `Composer` | `value`, `onChange`, `onSubmit`, `placeholder`, `disabled`, `sending`, `error`, `left` slot. Full-bleed `border-t bg-background-200 focus-within:bg-gray-100` strip with an outline-free borderless textarea (`min-h-[52px] px-gutter`), fused action bar carrying `left` and a `primary` `sm` `Send` button, and optional inline `red-900` error line. |
| `web/src/ui/Page.tsx` | `PageHeader`, `PageScroll` | `PageHeader` (`crumbs`, `title`, `right`, optional `children` for sub-rows) with `px-gutter py-4 border-b`; `PageScroll` full-width scrolling body with `p-gutter`. |
| `web/src/ui/Dot.tsx` | `Dot`, `StatusDot` | The sole circular primitive (`h-2 w-2 rounded-full`), supporting `grey`, `green`, `yellow`, `red`, and `pulse`. |

## Components and Patterns

- **Left rail (`Sidebar`)**: Fixed `260px` wide (`w-[260px]`), `px-edge` horizontal alignment throughout, zero vertical padding on group containers, groups divided by `border-b border-gray-400`. Brand header (`h-12`) shows `FactoryMark`, `Factory`, and mono build hash in `gray-700`. Optional `Update ready` row renders under the header when `Screen.top.pin.ready` is true, with a `Button` (`secondary`, `sm`) `Reload` and inline `red-900` refusal. The top navigation group holds `Board` (with live count) and `Usage` directly beneath it; hover uses `bg-gray-100`, active rows use `bg-gray-200` (with `font-medium` on `Board` and `Usage`). Below it, `Raised Hands` (dropping hands whose card is currently visible in an open project tree, hidden when empty) and collapsible project trees use `28px` (`h-7`) `label-12 gray-700` group headers and flush rows with medium mono card id, `HandIcon`, `StatusDot`, mono stage, and title `2px` (`mt-0.5`) underneath. Between the scrolling list and the connection footer, an always-visible `Quotas` block (`PageLink` to `/usage`, `border-t px-edge py-3 hover:bg-gray-100`) shows `Quotas` and `<dollars> / 7d` (from `App`'s shared 60s `useRecentBurn` poll, with ` + ?` on unknown cost or `? / 7d` before loaded), followed by flat rows for signed-in `5h` and `week` windows (`Claude 5h`, `Claude wk`, `Codex wk`, `Gemini 5h`, `3p wk`, etc.) powered by `quota.ts`: label, `4px` `ProgressBar` (green when projected `<= 100%`, amber when `> 100%` or reached, neutral otherwise; elapsed tick computed from `reset_ms` and `nowMs`), and right-aligned percent (amber when hot; stale rows show `stale` with an empty neutral bar and no tick). The `h-9` connection footer shows the status `Dot`, connection words, and a right-aligned `TextLink` `Enable alerts` while notification permission is unset.
- **Plain-text metadata tags**: Labels such as `design`, `sent to orchestrator`, `delegated`, `asks below`, card stages, backlog statuses, and build hashes render as plain grey text (`text-gray-900` or `text-gray-700`, monospace for ids/stages/statuses/hashes) with no pill chrome.
- **Shell banners and empty states**: Reconnecting and reloading/recovering banners render as plain `min-h-9 py-2` full-width bars (`border-b border-gray-400`) with a pulsing amber dot so wrapped words grow vertically without clipping. Waiting/disconnected states render as centred plain text on `background-100`, with no surrounding box.
- **Board (`/`)**: Full-width edge-to-edge workspace on `PageHeader` (`Workspace` crumb, `Board` title, `Segmented` project filter in `right`) and `PageScroll`. Opens with a four-cell `KpiStrip` (`Active Cards` with working seat count from non-carried rows, `Raised Hands` with expiring count, `Merge Queue` with slot/line counts, and `Landed Today` from `burn.daily`'s last day — marking `all projects` in its sub-line when filtered to one project). Below sit: `Raised Hands` (hidden when empty; one `List` of `Row` items linking to `cardPath(card, ask)`, or plain when a hand has no card, with `Dot`, id, single-line clamped words, meta line with age and `Cold`, inline refusal, and `sm` `Button` Triage + `OfferButtons` in `actions`), `Active Cards` (hidden when empty; every live row of the filtered projects with `StatusDot`, id, title, working seats at `xl+`, mono stage, and a 64px `strong` `ProgressBar` driven by `LiveMarks.progress`), `Merge Queue` (hidden when no queue is active; one `SectionHead` and per-project `List`s), and `Project Backlogs` (per-project counts, `Look back` with `secondary` `sm` `Button`s and `PageLink` running ids, and expandable area `List` rows with cards at `indent={1}`; unrun cards stay plain rows). Folded internal review cards do not appear as rows of their own; their working seats and open hands roll up to their parent card.
- **Usage page (`/usage`)**: Full-width edge-to-edge workspace on `PageHeader` (`Workspace` crumb, `Usage` title) and `PageScroll`. Shows any `burnError` refusal above a four-cell `KpiStrip` (`7-Day Spend`, `Average / Card`, `Prompt Cache Hit`, and `Cards Left` with the matching weekly pool's reset countdown). Directly beneath sits a compact, always-open bordered `Daily Spend & Landed Cards` block (hidden when `daily` is empty): one heading line carrying the hovered or today's date, dollars (` + ?` on unknown), and landed count, above a `72px` `ChartLayout` (`headroom`, `xAlign="columns"`) with 14 equal HTML bar columns (`gray-1000` for hovered/today, `gray-500` for others, no bar on `$0`, landed count directly above each bar in the 16px headroom so the top y-label aligns with the 100% bar ceiling, dates formatted in `Screen.tz_offset_min`). Below sits a two-column grid (`xl:grid-cols-2`, stacked below `xl`) with `Account Quotas` (`List` grouped by pool with `signed out`, `read <age> ago`, `Switch` `Button`, refusal under the header words, and `indent={1}` window rows using `quota.ts` with `6px` `ProgressBar` pace bars, elapsed ticks including 7-day `fable`, projected `→P%`, and reset countdowns) beside `Burn by Role` (`List` of roles sorted largest first with mono dollars, percentage share, and a `4px` monochrome `ProgressBar` beneath, `strong` on the top role and `neutral` on the rest).
- **Card Room layout (`/cards/<id>`)**: Full-height column with a full-width header (`PageHeader`) over a 50/50 split workspace (`SplitPane`) at every width: `Overview` with its anchored Raised Hands dock on the left, and auto-opened `SeatChat` on the right. Opening a folded review card's URL redirects to its parent card.
  - **Header, plain pipeline (`Pipeline`), and Seats switcher**: `PageHeader` (`border-b border-gray-400 px-gutter py-4`) carries the `Board / <project> / <card id>` breadcrumb (`TextLink` to `/`, optional `paused` label), `heading-24` title, horizontal plain-text pipeline (`Spec`, `Build`, `Verify`, `Land` for full/light cards; `Report`, `Done` for report cards; `Review`, `Done` for standalone review cards — no step is a button or underlined), and a compact `Seats` switcher beneath. Passed pipeline steps render ticked in `text-gray-700`, the current step bright in `font-medium text-gray-1000` with its mono sub-status, and to-come steps in `text-gray-700`. The `Seats` switcher renders one sharp-corner `h-[26px]` button per seat in order (the card's sessions, then the folded review card's foreman reading `review foreman`, its reviewers, and its other seats), each showing its status `Dot` (grey for `asking`, pulsing green for `working`, red for `error`), mono seat name, model label, and context in thousands; the open seat's button uses `border-gray-1000 bg-gray-200`, and clicking a seat opens its chat.
  - **Overview pane (`Overview`)**: Full-bleed `PaneList` of stage rows (`Row`, `pane`), one per stage in run order (`Spec`, `Build`, `Verify` / `Verify · Round <n>`, `Land`; `Report` / `Review` on such cards), each carrying a monochrome `✓` (passed), `✕` (failed round), or current-stage status `Dot` (red for red hand/seat error, else amber for open hand, else pulsing green while a seat works, else grey), label, summary, small `strong` `ProgressBar` beside the last `Build` stage's task count, mono commit hash on round stages, exact update count, `HH:MM · <duration>`, and chevron. Passed stages start folded and the current stage starts open; folded current stages take `active` (`bg-gray-200`). An unfolded stage renders, in order: (1) its live rows (current stage only, non-clickable `Row` at `indent={1}` with working reviewers at `indent={2}`: at Verify, `Checks`, `Test Suite` with mono percent, `<passed> / <total> passed · <failed> failed` counts, `4px` neutral `ProgressBar` and mono `latest` test, `review foreman`, and working reviewers; at other stages, working seats); (2) inline `Plan` deliverables inside the last `Build` stage (`Row` at `indent={1}`, current deliverable open by default, tasks at `indent={2}` with `✓`/`›`/`·` marks); (3) inline round findings inside `Verify · Round <n>` / `Review · Round <n>` (`Row` at `indent={1}` with `#n`, state, summary, `file:line · reviewer`, unfolding markdown proof at `indent={2}`, or plain non-foldable row when proof was shed; hidden while that round's triage hand is open); and (4) human-facing updates from Spec, Build, and Review (including the answered-ask fold and `N earlier updates` note) at the stage body indent. The stage list scrolls in `PinnedScroll`: it stays at the bottom while the reader is there and shows the scroll-to-bottom button above the Raised Hands dock while scrolled up.
  - **Raised Hands dock (`HandCard`, `RoomHands`, `TriagePanel`)**: Anchored in one fixed spot at the bottom of the Overview pane (`max-h-[60%] overflow-y-auto border-t border-gray-400 pb-3`), never docked above a seat composer. Holds every open hand of the card: one shown whole (named by `?hand=`, else oldest by `raised_ms`), the others folded to one monochrome `border-gray-500 bg-gray-100` line each (`px-gutter`); clicking a folded line opens it and folds the previous whole hand. Action buttons (`OfferButtons` and triage's `Send`) use `ui/Button` (`sm`). On a seat's `answer` ask, typed `choices` render as numbered clickable answer buttons with the seat's pick mildly highlighted (`border-gray-700 bg-gray-200`). An `approve-brief` hand with a typed `brief` shows `What changes`, numbered `Decisions`, a folded `Code shape <count>` row, and its mock link. A `triage` ask renders in the same `HandCard` with live tally, `Button` (`primary`, `sm`) `Send`, collapsible markdown proofs, and per-finding `Fix / Decline / Foreman decides` switches; its chosen answers, `sending` state, and `sendError` live in a module-level per-ask store in `Triage.tsx` so all three survive folding, seat switches, and leaving the card's page until the ask closes.
  - **Auto-opened Seat Chat (`SeatChat`) and stream (`RoomStream`)**: Opens automatically on the right pane for the seat of the hand shown whole when a seat raised it, else the current pipeline step's seat (`planner` on `spec` for a two-room card, `review foreman` on `verify` when the folded review has one, else `foreman`; `foreman` on report and review cards), following live until Andrei picks a seat in `Seats` (returning to live following when a hand is opened or the page is left). Its pane header (`border-b border-gray-400 px-gutter py-3`) shows the seat's status `Dot`, mono name, room (`Spec`, `Build`, `Review`), mono context (`ctx used/window`), and cache state (`warm`, `warm Nm`, `Cold`, `cold`). Below sits `RoomStream` (`px-gutter py-2` inside `PinnedScroll`, pinned to the bottom and showing the scroll-to-bottom button above the composer while scrolled up, no header of its own; messages oldest first, Andrei's monochrome `bg-gray-100 border-gray-400` block, collapsed `×N` engine markers, ended turns, and inline live `ActivityTracker`). At the bottom sits `SeatModelPanel` (`border-t border-gray-400 px-gutter py-3` when open) above the full-bleed `ui/Composer` with a `Button` (`secondary`, `sm`) model/effort trigger in its `left` slot, primary `Send` at the right, per-place drafts, and placeholder `Answer <seat>…` while that seat's hand shows whole (else `Message <seat>…`).
  - **Markdown (`Markdown`)**: Rendered through `react-markdown` with no raw HTML and an in-tree remark step that autolinks bare `http://` and `https://` URLs in prose (trimming trailing punctuation, skipping links and code). Links open in a new tab (`noreferrer`) with `text-gray-1000 underline decoration-gray-500 underline-offset-2 hover:decoration-gray-1000`; inline code uses `bg-gray-100 px-1 font-mono text-label-13 text-gray-1000`; fenced blocks use `border border-gray-400 bg-gray-100 p-3 font-mono text-label-13` with square corners; blockquotes use `border-l border-gray-400 pl-3 text-gray-900`.

## What Is Not Used

- Rounded tinted status pills, colored stage pills, count pills, or bordered tag chips (the square `Raised Hand` header label stays)
- Nested card boxes, arbitrary tile grids, or boxed collapsible drawers (`KpiStrip` is the one allowed bordered tile row; Andrei's stream message block is the one filled row in the stream, beside the docked Raised Hand card)
- Tinted row backgrounds or coloured left accent bars (Andrei's stream message block stays monochrome `gray-100`)
- Vertical padding on bordered containers holding full-bleed rows
- Negative margins (`-mx-*`, `-mt-*`, etc.) outside `web/src/ui/`
- Stretched `<text>` inside `<svg preserveAspectRatio="none">`
- `className` props on `web/src/ui/` primitives, or page-local row/list class constants on newly built pages
- Drop shadows, inner highlights, or dot glows
- Border radii anywhere outside `web/src/ui/Dot.tsx`
- Any colour outside the tokens in `web/src/index.css` (no indigo, violet, sky, emerald, rose, or arbitrary opacity whites)
