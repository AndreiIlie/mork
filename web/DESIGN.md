# Web Design System

Restrained developer-tool visual system for the web UI, built on Cold Geist dark tokens (Vercel Geist neutral greys), Geist Sans and Geist Mono typography, monochrome neutral surfaces, crisp hairline borders, sharp `0px` corners, flat rows, high-contrast primary actions, and colour reserved strictly for status dots, Card Tree stage marks, reviewer finding counts, the inline Raised Hand label, errors, cache-expiry countdowns, and quota progress pace.

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
| `gray-100` | `hsl(0 0% 9%)` | Row hover fill, button hover fill, composer focus fill, the operator's stream message block |
| `gray-200` | `hsl(0 0% 12%)` | Active nav/card/row fill, selected segment, progress bar track |
| `gray-300` | `hsl(0 0% 16%)` | Subtle interactive surface step (reserved in Geist scale) |
| `gray-400` | `hsl(0 0% 18%)` | All 1px hairline borders, dividers, segmented control frame, the operator's stream message block border, scrollbar thumb |
| `gray-500` | `hsl(0 0% 27%)` | Idle/disconnected status dot, bordered button hover border, scrollbar thumb hover |
| `gray-600` | `hsl(0 0% 53%)` | Upcoming Card Tree stage labels |
| `gray-700` | `hsl(0 0% 56%)` | Tertiary text (idle state, timestamps, sidebar build/stage/group text, backlog statuses, chart axes, helper notes) |
| `gray-900` | `hsl(0 0% 63%)` | Secondary text (section counts, metadata, roles, subtitles, neutral progress fill, ghost button label, text link) |
| `gray-950` | `hsl(0 0% 78%)` | Primary button hover fill and border, visited stage labels and context ring fill |
| `gray-1000` | `hsl(0 0% 93%)` | Primary text, card ids, headings, focus outline, primary button background, strong progress fill, elapsed tick |
| `gray-alpha-400` | `#ffffff24` | Text selection highlight (`selection:bg-gray-alpha-400`) |
| `red-700` | `hsl(358 75% 59%)` | Positive kept finding count, failed Card Tree stage mark, red status dot (error / red hand) and red `Raised Hand` header label |
| `red-900` | `hsl(358 100% 69%)` | Error text (inline command refusals) |
| `amber-700` | `hsl(39 100% 57%)` | Positive reported finding count, yellow/amber status dot (waiting hand, reconnecting/reloading banner, connecting link), amber `Raised Hand` header label, over-pace quota bar fill and percent text |
| `amber-900` | `hsl(39 90% 50%)` | Cache-expiry countdown text (`cold in Xm`) |
| `green-700` | `hsl(131 41% 46%)` | Passed Card Tree stage mark, green status dot (connected link, actively working seat or card) and on-pace quota bar fill |

Two spacing tokens are declared in `@theme`:

| Token | Value | Use |
|---|---|---|
| `--spacing-gutter` (`p-gutter`, `px-gutter`) | `24px` | Outer gutter for pages, page headers, pane headers, full-bleed pane rows, and composer |
| `--spacing-edge` (`p-edge`, `px-edge`) | `16px` | Inner edge padding for the left rail, bordered list rows, and cards |

Row vertical padding (`py-3`, `12px`) and gap (`gap-3`, `12px`) use Tailwind's standard scale.

### Where Colour Is Allowed

The following elements use colour.

1. **Status dots (`Dot` / `StatusDot`) and favicon badge**: `red-700`, `amber-700`, `green-700`, and neutral `gray-500`. Working cards pulse gently with `.animate-pulse-soft` (disabled under `prefers-reduced-motion`). In the Card Room, yellow dots are omitted from stream message hands and seat buttons/headers (`asking` renders `gray-500`); the Card Tree's current-stage row carries one status dot (red, amber, pulsing green, or grey). The browser tab favicon mirrors `MorkMark` (`background-100` / `gray-1000`) and adds a top-right circular `red-700` or `amber-700` badge dot when hands wait on the operator.
2. **Raised Hand label** sits in the message header. Its square label uses amber, red, or grey for a hand sent to the orchestrator.
3. **Error text**: `red-900` for inline command refusals and error messages.
4. **Cache-expiry countdowns (`Cold`)**: `amber-900` monospace text (`cold in Xm`), with no pill or row tint.
5. **Quota progress pace (`ProgressBar` and over-pace percent)**: `green-700` fill while projected window end is within 100%, `amber-700` fill and `amber-700` percent text when projected past 100% or reached.
6. **Card Tree stage marks** use `green-700` for ✓ and `red-700` for ✕.
7. **Reviewer finding counts** use `amber-700` for positive reported counts and `red-700` for positive kept counts. The slash is `gray-1000` and zero is `gray-700`. Hovering explains the counts.

Everything else stays strictly monochrome.

## Type Scale

Geist's named scale, exposed as `text-<name>` utilities (no intermediate sizes are added; every text uses the nearest named token):

| Token | Size / Line height | Weight | Letter spacing | Use |
|---|---|---|---|---|
| `heading-24` | `24px / 32px` | `600` | `-0.96px` | Page titles (Mork Console, project name, Settings, card title) |
| `heading-16` | `16px / 24px` | `600` | `-0.32px` | Centered state headings (`Not connected`, `Connecting…`) |
| `heading-14` | `14px / 20px` | `600` | `-0.28px` | Section headings (`Raised Hands`, `Look back`, `Merge Queue`, `Backlog`, `Daily Spend & Landed Cards`, `Account Quotas`, `Burn by Role`), brand title |
| `label-14` | `14px / 20px` | `400` | `0` | Row titles, primary nav items, root text size |
| `label-13` | `13px / 16px` | `400` | `0` | Card ids, metadata, working seats, project names, segmented controls, `Button` md, `TextLink`, banners, header controls |
| `label-12` | `12px / 16px` | `400` | `0` | Counts, timestamps, stages, statuses, countdowns, sidebar items, `Button` sm, quotas, chart axes, helper notes |
| `copy-14` | `14px / 20px` | `400` | `0` | Raised hand body words, composer input, placeholder/waiting descriptions |
| `copy-13` | `13px / 18px` | `400` | `0` | Inline error text under a hand |

## Radius, Borders, Spacing Rules, and Focus

- **Radius**: `0px` everywhere except circular `8px` status dots (`rounded-full` inside `web/src/ui/Dot.tsx`).
- **Borders**: `1px solid var(--color-gray-400)` (`border-gray-400` / `divide-gray-400`) for structural hairlines: sidebar edge, header/footer rules, banners, row lists, segmented controls, and secondary buttons; `border-gray-500` for bordered button hover.
- **Outer gutter and inner edge**: One outer gutter (`--spacing-gutter`, `24px`) for pages, page headers, Card Room panes, pane rows, and composer; one inner edge (`--spacing-edge`, `16px`) for the left rail, bordered list rows, and cards.
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
| `web/src/ui/Button.tsx` | `Button` | Primary, secondary and ghost tiers. Action buttons share the 28px outer height from `--spacing-control`. The default uses `label-13`, compact buttons use `label-12`. A square button has equal width and height. A pressed secondary button uses `bg-gray-200 border-gray-1000` and sets `aria-pressed`. |
| `web/src/ui/TextLink.tsx` | `TextLink` | `to` (renders `PageLink`), or `onClick` / `disabled` / `title` (renders button). Plain `label-13` `gray-900`, hover `gray-1000` with underline (`3px` offset), no box or padding. |
| `web/src/ui/Segmented.tsx` | `Segmented` | `options`, `value`, `onChange`, optional `label` and per-option `disabled`. The shared 28px control height includes borders. The active segment uses `bg-gray-200 font-medium text-gray-1000`. |
| `web/src/ui/List.tsx` | `List`, `PaneList`, `Row` | `List` (bordered, zero padding) and `PaneList` (full-bleed edge-to-edge hairlines, zero padding). `Row` takes `indent` (`0 \| 1 \| 2`), `pane` (`boolean`), `onClick`, `expanded` (`aria-expanded`), `to` (`PageLink`), `actions` (sibling slot outside the trigger so buttons never nest inside `<button>` or `<a>`), and `active` (`bg-gray-200`). Interactive rows highlight `hover:bg-gray-100`; plain rows have no hover. |
| `web/src/ui/ProgressBar.tsx` | `ProgressBar` | `percent` (0–100), `tone` (`"neutral"` `gray-900`, `"ok"` `green-700`, `"hot"` `amber-700`, `"strong"` `gray-1000`), `height` (`2 \| 4 \| 6`), optional `tick` (0–100, 1px `gray-1000` line extending 2px above and below the track). |
| `web/src/ui/ChartLayout.tsx` | `ChartLayout` | `yLabels`, `xLabels` (plain strings or `{ full, short }` pairs switching to `short` below `lg`), `height` (`72 \| 156`, default `156`), `headroom` (pads both the y-labels and plot area by `16px` at top so bar value labels fit above the bar ceiling), `xAlign` (`"between" \| "columns"` to centre labels under equal columns), `children` (the stretched plot slot). Renders HTML `font-mono label-12 tabular-nums` labels around a bordered plot area. |
| `web/src/ui/Kpi.tsx` | `KpiStrip` | `items` (`label`, `value`, `sub`). The one allowed bordered tile row. At `lg` and above, the Console has four equal columns and Settings Usage has three. Below `lg`, both use two columns, with the final Usage cell spanning both. |
| `web/src/ui/Section.tsx` | `SectionHead` | `title`, optional `note` (mono count/note), optional `right` slot. Shared section heading line above lists. |
| `web/src/ui/Menu.tsx` | `Menu` | Square More button and a right-aligned list of actions. The menu supports keyboard selection and closes on Escape, outside click or selection. |
| `web/src/ui/TextInput.tsx` | `TextInput` | Single-line field with a 30px height, hairline border and `label-13` text. Used for card titles and the words accompanying a card action. |
| `web/src/ui/PinnedScroll.tsx` | `PinnedScroll` | `children`, optional `scrollRef` (receives the scroll box, for a caller restoring its place after prepending rows), optional `keepRef` (set by that caller once it restored the place, so the next resize keeps it instead of re-pinning). A `relative flex-1` column whose `overflow-y-auto` box stays at its bottom while the reader is within `24px` of it, re-pinning on every resize of the box or its content (late images above or below the view, new rows, fonts, a neighbouring dock). Scrolling up, or a resize after the reader's own click inside it (an unfold), keeps the reader's place and unpins unless the pane is still at its bottom; an unpinned pane pins again once a scroll or resize leaves it at its bottom. While scrolled up it shows a centred `Button` (`secondary`, `sm`, `square`) with `ArrowDownIcon`, `aria-label` `Scroll to bottom`, `12px` above the pane's bottom edge; a click jumps to the bottom and pins again. |
| `web/src/ui/Composer.tsx` | `Composer` | optional `attachments`, `onPasteImages`, and `onRemove` for Seat Chat, `value`, `onChange`, `onSubmit`, optional `onSendNow`, `placeholder`, `disabled`, `sending`, `error`, `left` slot. Full-bleed `border-t bg-background-200 focus-within:bg-gray-100` strip with a `px-gutter pt-3 pb-3` label wrapping an outline-free, borderless, unpadded textarea that grows with its text from 2 rows to 8 lines (`max-h-[160px]`), refitting when the pane's width changes and named by its placeholder, then scrolls inside that fixed padding, fused action bar carrying `left` and a `primary` `sm` `Send` button, and optional inline `red-900` error line. Clipboard text takes precedence when a paste also includes an image. Pasted thumbnails sit above the field with separate preview and remove controls. |
| `web/src/ui/ImagePreview.tsx` | `ImageThumbnail` | `url: string`, `name: string`, optional `draft: boolean`. Draft thumbnails are 64px high and sent thumbnails are 96px high. Both open the Card Room image preview. |
| `web/src/ui/Page.tsx` | `PageHeader`, `PageScroll` | `PageHeader` (`crumbs`, `title`, `right`, optional `children` for sub-rows) with `px-gutter py-4 border-b`; `PageScroll` full-width scrolling body with `p-gutter`. |
| `web/src/ui/Dot.tsx` | `Dot`, `StatusDot` | The sole circular primitive (`h-2 w-2 rounded-full`), supporting `grey`, `green`, `yellow`, `red`, and `pulse`. |

## Components and Patterns

- **Sidebar.** The Mork link at the top of the 260px rail opens the Console and shows the number of hands waiting on the operator. Each project name and count link to its overview. Its chevron independently expands or folds the live cards. A card row shows its status, id, stage, title and any waiting hand. The active destination uses `bg-gray-200`.
- **Sidebar footer.** The Resource tracker shows CPU percent and RAM used/total in whole GiB above the quotas when enabled. Up to three signed-in quota windows with current readings link to Settings Usage. Reached windows appear first, followed by those forecast to run out before reset, soonest first. Remaining windows rank by projected usage at reset, falling back to current usage. Each row shows the pool and window, usage, the available reset countdown and a 2px pace bar with an elapsed tick. Connection status takes their place when the link drops. Settings sits at the bottom with Update ready or the build number.
- **Plain-text metadata tags**: Labels such as `design`, `sent to orchestrator`, card stages, backlog statuses, and build hashes render as plain grey text (`text-gray-900` or `text-gray-700`, monospace for ids/stages/statuses/hashes) with no pill chrome.
- **Shell banners and empty states**: Reconnecting and reloading/recovering banners render as plain `min-h-9 py-2` full-width bars (`border-b border-gray-400`) with a pulsing amber dot so wrapped words grow vertically without clipping. Waiting/disconnected states render as centred plain text on `background-100`, with no surrounding box.
- **Mork Console (`/`).** The four figures show Active Cards with working seat count, Landed Today, 7-Day Spend and Average / Card across projects. Raised Hands lists asks waiting on the operator and links to their Card Rooms. Cardless asks keep their action buttons. The Daily Spend & Landed Cards chart appears below.
- **Console commands.** The composer stays at the bottom and runs slash commands in main. The latest command and its answer appear above it in a scrollable block up to 40vh tall. Close or Escape dismisses the answer. An empty answer shows `ok`. `/help` shows the command list with the composer below. Plain messages and command errors show an inline refusal. The draft stays editable.
- **Project overview (`/projects/<name>`).** Auto delegation and New card sit in the header. New card opens the title and description form for this project. Merge Queue shows the base, slot and waiting cards, including an empty slot. Look back shows each pass, its mode when scheduled or auto, its last run and its action. A running pass links to its card. Backlog groups cards by area and keeps the expandable Start controls for mode, Visual, No planner, Delegation and carried cards. The project crumb in a Card Room returns to its overview.
- **Settings (`/settings`).** Escape from any tab returns to the last address outside Settings, or the Mork Console on a direct visit. Inputs and modals keep their own Escape handling. General shows the browser notification permission and offers Enable alerts while permission is unset. Resource tracker defaults to On and saves this browser's choice. Updates shows the running build, the available update and Reload, followed by dated changelog entries, newest first, loading earlier entries on scroll.
- **Settings Usage (`/settings/usage`, also `/usage`).** Shows any `burnError` refusal above a three-cell `KpiStrip` (`7-Day Spend`, `Average / Card`, and `Cards Left` with the matching weekly pool's reset countdown). Directly below the summary, `Prompt Cache Hit` lists one full-width row per CLI and launch model, with its seven-day percentage or `?` when prompt counts are unavailable. Next comes a compact, always-open bordered `Daily Spend & Landed Cards` block (hidden when `daily` is empty): one heading line carrying the hovered or today's date, dollars (` + ?` on unknown), and landed count, above a `72px` `ChartLayout` (`headroom`, `xAlign="columns"`) with 14 equal HTML bar columns (`gray-1000` for hovered/today, `gray-500` for others, no bar on `$0`, landed count directly above each bar in the 16px headroom so the top y-label aligns with the 100% bar ceiling, dates formatted in `Screen.tz_offset_min`). Below sits a two-column grid (`xl:grid-cols-2`, stacked below `xl`) with `Account Quotas` (`List` grouped by pool with `signed out`, `read <age> ago`, `Switch` `Button`, refusal under the header words, and `indent={1}` window rows using `quota.ts` with `6px` `ProgressBar` pace bars, elapsed ticks including 7-day `fable`, projected `→P%`, and reset countdowns) beside `Burn by Role` (`List` of roles sorted largest first with mono dollars, percentage share, and a `4px` monochrome `ProgressBar` beneath, `strong` on the top role and `neutral` on the rest).
- **Card Room (`/cards/<id>`).** The two-line header spans the room. Its first line shows the breadcrumb, stage status, Delegation, Visual, Preview, Pause or Resume, and More. Header controls share an outer height of 28px. The second line shows the card title in `heading-24`. More contains Back to brief, Reopen mock and Close card. Back to brief and Close open an inline text field. Opening a folded review card redirects to its parent.
  - **Card Tree.** The independently scrolling 360px tree lists visited stages in run order, then upcoming stages in `gray-600`, including when selected. Completed stage labels use `gray-950`. Each stage shows its summary and mark. Each seat appears once under its latest stage, with children indented beneath their parent. The selected row uses `bg-gray-200`. Seat names, hand marks, model, effort and context wrap within the row. Context uses a neutral 12px ring with a 1.5px stroke. It fills clockwise and shows used/window on hover. The tree, composer and model picker use the same display name for each model. A finished reviewer shows its reported count until the verdict, then kept/reported. Checks and Test Suite appear beneath the current Verify stage.
  - **Stage detail.** Selecting a stage fills the remaining width with its label, summary, commit when available, time and duration. Spec lists standing decisions in order. The latest Build shows deliverables and tasks. Verify shows live checks and suite progress, the round’s reviewer counts, and findings with expandable proofs. While triage is open, its findings appear in the chat. An upcoming stage reads `Not started yet.`
  - **Inline asks** appear in the chat on the message that raised them. Each answer has a square numbered button beside its label, with the seat's recommendation beneath its choice. Plan approval shows What changes, Decisions, folded Code shape and the mock link. Triage keeps its finding choices, proofs, tally and Send button together. Other gates show their available commands. Earlier open asks stay visible above the loaded messages. A Raised Hands row or notification opens the seat and scrolls to its message. The seat button carries a hand mark while its chat holds an open ask, grey when all were sent to an orchestrator.
  - **Auto-opened Seat Chat (`SeatChat`) and stream (`RoomStream`)**: Opens automatically on the right pane for the chat holding the oldest ask waiting on the operator, else the current pipeline step's seat (`planner` on `spec` for a two-room card, `review foreman` on `verify` when the folded review has one, else `foreman`; `foreman` on report cards and `review foreman` on review cards), following live until the operator selects a row in the Card Tree. Its pane header (`border-b border-gray-400 px-gutter py-3`) shows the seat's status `Dot`, mono name, room (`Spec`, `Build`, `Review`), the same context ring as the tree, and cache state (`warm`, `warm Nm`, `Cold`, `cold`). Below sits `RoomStream` (`px-gutter py-2` inside `PinnedScroll`, pinned to the bottom and showing the scroll-to-bottom button above the composer while scrolled up, no header of its own; messages oldest first, the operator's monochrome `bg-gray-100 border-gray-400` block, collapsed `×N` engine markers, ended turns, and inline live `ActivityTracker`). Addressed messages show `Unread` beside their timestamp until the recipient reads them. An unread message offers `Send now` while its recipient can be interrupted. At the bottom sits `SeatModelPanel` (`border-t border-gray-400 px-gutter py-3` when open) above the full-bleed `ui/Composer` with a `Button` (`secondary`, `sm`) model/effort trigger in its `left` slot, secondary `Send now` beside primary `Send` at the right, per-place drafts, and placeholder `Answer <seat>…` while that seat has an open ask in its chat (else `Message <seat>…`).
  - **Composer commands.** `/help` shows a scrollable command list in place of the conversation, with the composer available below. Close or Escape returns to chat. `/open <card>` opens a Card Room, and `/usage` opens Settings Usage. A leading `@name` addresses the message. Command errors appear below Send and the draft stays editable.
  - **Image preview.** Clicking a draft or message thumbnail opens a modal over the Card Room. The image starts fitted. The toolbar shows its name, zoom controls, scale, Fit and a close button. Drag pans, wheel and pinch zoom, and Escape returns focus to the thumbnail.
  - **Markdown (`Markdown`)**: Rendered through `react-markdown` with no raw HTML and an in-tree remark step that autolinks bare `http://` and `https://` URLs in prose (trimming trailing punctuation, skipping links and code). Links open in a new tab (`noreferrer`) with `text-gray-1000 underline decoration-gray-500 underline-offset-2 hover:decoration-gray-1000`; inline code uses `bg-gray-100 px-1 font-mono text-label-13 text-gray-1000`; fenced blocks use `border border-gray-400 bg-gray-100 p-3 font-mono text-label-13` with square corners; blockquotes use `border-l border-gray-400 pl-3 text-gray-900`.

## What Is Not Used

- Rounded tinted status pills, colored stage pills, count pills, or bordered tag chips (the square `Raised Hand` header label stays)
- Nested card boxes, arbitrary tile grids, or boxed collapsible drawers (`KpiStrip` is the one allowed bordered tile row; the operator's stream message block is the one filled row in the stream)
- Tinted row backgrounds or coloured left accent bars (the operator's stream message block stays monochrome `gray-100`)
- Vertical padding on bordered containers holding full-bleed rows
- Negative margins (`-mx-*`, `-mt-*`, etc.) outside `web/src/ui/`
- Stretched `<text>` inside `<svg preserveAspectRatio="none">`
- `className` props on `web/src/ui/` primitives, or page-local row/list class constants on newly built pages
- Drop shadows, inner highlights, or dot glows
- Border radii anywhere outside `web/src/ui/Dot.tsx`
- Any colour outside the tokens in `web/src/index.css` (no indigo, violet, sky, emerald, rose, or arbitrary opacity whites)
