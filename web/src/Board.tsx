import { useState } from "react";
import { NewCard, StartStrip } from "./BoardForms";
import { ownHands } from "./alerts";
import { RaisedHands } from "./Hands";
import { cardTarget, coldMinutes, dollars, duration, plural, shortAge, workingSeats } from "./format";
import { ChevronIcon, GitMergeIcon } from "./icons";
import { RefusalLine, useHandOffers } from "./offers";
import { PageLink, cardPath } from "./route";
import type {
  BoardGroup,
  BoardRow,
  PlaceView,
  BoardView,
  HandView,
  LookbackRead,
  MergeQueue,
  ProjectView,
  ReadView,
  RecentBurn,
  Screen,
} from "./types";
import { Button } from "./ui/Button";
import { StatusDot } from "./ui/Dot";
import { type Kpi, KpiStrip } from "./ui/Kpi";
import { List, Row } from "./ui/List";
import { PageHeader, PageScroll } from "./ui/Page";
import { ProgressBar } from "./ui/ProgressBar";
import { SectionHead } from "./ui/Section";
import { Segmented } from "./ui/Segmented";

const ALL = "all";
const ID = "min-w-[76px] max-w-[220px] shrink-0 truncate font-mono text-label-13 font-medium text-gray-1000";
const MONO = "font-mono text-label-12 tabular-nums";

function kpis(
  screen: Screen,
  boards: BoardView[],
  hands: HandView[],
  burn: RecentBurn | null,
  nowMs: number,
  filtered: boolean,
): Kpi[] {
  const rows = boards.flatMap((b) => b.live.cards);
  const seats = rows.filter((r) => r.room === undefined).reduce((n, r) => n + workingSeats(r, screen.rooms).length, 0);
  const expiring = hands.filter((h) => coldMinutes(h.cold_at_ms, nowMs) !== null).length;
  const inSlot = boards.filter((b) => b.merge.slot !== null).length;
  const inLine = boards.reduce((n, b) => n + b.merge.in_line, 0);
  const today = burn?.daily?.[burn.daily.length - 1];
  const todaySpend = today === undefined ? "" : `${dollars(today.cents)}${today.unknown ? " + ?" : ""}`;
  return [
    { label: "Active Cards", value: String(rows.length), sub: `${seats} ${plural(seats, "seat", "seats")} working now` },
    { label: "Raised Hands", value: String(hands.length), sub: expiring > 0 ? `${expiring} expiring soon` : "none expiring soon" },
    { label: "Merge Queue", value: String(inSlot + inLine), sub: `${inSlot} in slot · ${inLine} in line` },
    {
      label: "Landed Today",
      value: today === undefined ? "?" : String(today.landed),
      sub: filtered
        ? today === undefined
          ? "all projects"
          : `${todaySpend} spent · all projects`
        : today === undefined
          ? ""
          : `${todaySpend} spent today`,
    },
  ];
}

function ActiveCards({ boards, screen }: { boards: BoardView[]; screen: Screen }) {
  const rows = boards.flatMap((b) => b.live.cards);
  if (rows.length === 0) return null;
  return (
    <section className="mt-gutter">
      <SectionHead title="Active Cards" note={`${rows.length} running`} />
      <List>
        {rows.map((r) => {
          const seats = workingSeats(r, screen.rooms);
          const labels = [...new Set(seats)].join(", ");
          return (
            <Row key={r.id} to={cardPath(cardTarget(r))}>
              <StatusDot light={r.live?.light ?? null} working={seats.length > 0} />
              <span title={r.id} className={ID}>
                {r.id}
              </span>
              <span className="min-w-0 flex-1 truncate text-label-14 text-gray-1000">{r.title}</span>
              <span className="hidden shrink-0 text-label-12 text-gray-700 xl:block">{labels}</span>
              <span className={`w-[132px] shrink-0 text-right ${MONO} text-gray-900`}>{r.live?.stage}</span>
              <span className="w-16 shrink-0">
                <ProgressBar percent={r.live?.progress ?? 0} tone="strong" />
              </span>
            </Row>
          );
        })}
      </List>
    </section>
  );
}

function MergeQueueList({ project, queue, nowMs }: { project: string; queue: MergeQueue; nowMs: number }) {
  const place = `w-[76px] shrink-0 whitespace-nowrap ${MONO}`;
  return (
    <div className="mt-4 first:mt-0">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <GitMergeIcon className="text-gray-900" />
        <span className="text-label-13 text-gray-1000">{project}</span>
        <span className="text-label-13 text-gray-700">→</span>
        <span className="font-mono text-label-12 text-gray-1000">{queue.base}</span>
        <span className={`ml-auto ${MONO} text-gray-700`}>
          {queue.slot !== null ? "1 in slot" : "slot free"} · {queue.in_line} in line
        </span>
      </div>
      <List>
        {queue.slot !== null ? (
          <Row to={cardPath(cardTarget(queue.slot[0]))}>
            <span className={`${place} text-gray-1000`}>In slot</span>
            <span title={queue.slot[0].id} className={ID}>
              {queue.slot[0].id}
            </span>
            <span className="min-w-0 flex-1 truncate text-label-14 text-gray-1000">{queue.slot[0].title}</span>
            <span className={`shrink-0 ${MONO} text-gray-900`}>{queue.slot[1]}</span>
          </Row>
        ) : (
          <Row>
            <span className={`${place} text-gray-1000`}>In slot</span>
            <span className="text-label-13 text-gray-700">Nobody in the slot</span>
          </Row>
        )}
        {queue.waiting.map(([r, at], i) => (
          <Row key={r.id} to={cardPath(cardTarget(r))}>
            <span className={`${place} text-gray-700`}>#{i + 1} in line</span>
            <span title={r.id} className={ID}>
              {r.id}
            </span>
            <span className="min-w-0 flex-1 truncate text-label-14 text-gray-900">{r.title}</span>
            <span className={`shrink-0 ${MONO} text-gray-900`}>waiting {duration(nowMs - at)}</span>
          </Row>
        ))}
      </List>
    </div>
  );
}

const READ_LABEL: Record<LookbackRead, string> = {
  dream: "Dream",
  pulse: "Pulse",
  "deep-sweep": "Deep sweep",
};

function lastRunText(row: ReadView, nowMs: number): string {
  if (row.last === null) return "none yet";
  const age = shortAge(nowMs - row.last.ended_ms);
  if (row.last.commits === null) return age;
  return `${age}, ${row.last.commits} ${plural(row.last.commits, "commit", "commits")} since`;
}

function LookBack({
  project,
  rows,
  nowMs,
  live,
  pending,
  onPress,
}: {
  project: string;
  rows: ReadView[];
  nowMs: number;
  live: boolean;
  pending: string | null;
  onPress: (label: string, read: LookbackRead) => void;
}) {
  const running = rows.some((r) => r.running !== null);
  return (
    <>
      <span className="text-label-12 text-gray-700">Look back</span>
      {rows.map((r) => {
        const label = READ_LABEL[r.read];
        const mode = r.mode !== "manual" && <span className="text-label-12 text-gray-700">{r.mode}</span>;
        if (r.running !== null) {
          return (
            <span key={r.read} className="flex items-center gap-2 text-label-12 text-gray-900">
              {label} {mode} running
              <PageLink to={cardPath(r.running)} className="font-mono text-gray-1000 hover:underline">
                {r.running}
              </PageLink>
            </span>
          );
        }
        const last = lastRunText(r, nowMs);
        return (
          <span key={r.read} className="flex items-center gap-2">
            <span className="text-label-12 text-gray-700">{running ? `${label} ${last}` : last}</span>
            {mode}
            {!running && (
              <Button
                tier="secondary"
                size="sm"
                disabled={!live || pending !== null}
                onClick={() => onPress(label, r.read)}
                title={`Look back: ${project} ${label.toLowerCase()}`}
              >
                {pending === label ? `${label}…` : label}
              </Button>
            )}
          </span>
        );
      })}
    </>
  );
}

interface StartSelection {
  id: string | null;
  view?: PlaceView;
  select: (id: string | null) => void;
}

function AreaRows({ group, startable, start, live }: { group: BoardGroup; startable: boolean; start: StartSelection; live: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Row onClick={() => setOpen((v) => !v)} expanded={open}>
        <ChevronIcon open={open} className="text-gray-900" />
        <span className="text-label-14 text-gray-1000">{group.name}</span>
        <span className={`ml-auto ${MONO} text-gray-900`}>{group.count}</span>
      </Row>
      {open &&
        group.cards.map((r: BoardRow) => (
          <div key={r.id}>
            <Row
              indent={1}
              to={r.ran ? cardPath(cardTarget(r)) : undefined}
              actions={
                startable && !r.room ? (
                  <Button tier="secondary" size="sm" pressed={start.id === r.id} disabled={!live} onClick={() => start.select(start.id === r.id ? null : r.id)}>
                    Start
                  </Button>
                ) : undefined
              }
            >
              <span title={r.id} className="min-w-[76px] max-w-[220px] shrink-0 truncate font-mono text-label-13 text-gray-900">
                {r.id}
              </span>
              <span className="min-w-0 flex-1 truncate text-label-14 text-gray-1000">{r.title}</span>
              {r.status !== undefined && <span className={`shrink-0 ${MONO} text-gray-700`}>{r.status}</span>}
            </Row>
            {startable && !r.room && start.id === r.id && <StartStrip card={r} view={start.view} live={live} onDone={() => start.select(null)} />}
          </div>
        ))}
    </>
  );
}

function ProjectBacklog({
  start,
  board,
  lookback,
  nowMs,
  live,
}: {
  board: BoardView;
  start: StartSelection;
  lookback: ProjectView | undefined;
  nowMs: number;
  live: boolean;
}) {
  const { pending, refusals, press } = useHandOffers();
  const backlog = board.areas.reduce((n, g) => n + g.count, 0);
  const groups = [...board.areas, board.done].filter((g) => g.count > 0);
  return (
    <div className="mt-4 first:mt-0">
      <div className="mb-2 flex min-h-[26px] flex-wrap items-center gap-x-3 gap-y-2">
        <span className="text-label-13 text-gray-1000">{board.project}</span>
        <span className={`${MONO} text-gray-700`}>
          {backlog} backlog · {board.done.count} {board.done.name.toLowerCase()}
        </span>
        <div className="ml-auto flex flex-wrap items-center gap-x-4 gap-y-2">
          <span className="flex items-center gap-2">
            <span className="text-label-12 text-gray-700">Auto delegation</span>
            <Segmented
              compact
              options={["Off", "On"] as const}
              value={board.auto ? "On" : "Off"}
              disabled={() => !live || !!pending[board.project]}
              onChange={(v) => {
                const on = v === "On";
                if (on === board.auto) return;
                press(board.project, { label: "Auto delegation", command: { auto: { project: board.project, off: !on } } });
              }}
            />
          </span>
          {lookback !== undefined && lookback.rows.length > 0 && (
            <LookBack
              project={board.project}
              rows={lookback.rows}
              nowMs={nowMs}
              live={live}
              pending={pending[board.project] ?? null}
              onPress={(label, read) => press(board.project, { label, command: { lookback: { project: board.project, read } } })}
            />
          )}
        </div>
      </div>
      <RefusalLine refusal={refusals[board.project] ?? null} className="mb-2" />
      {groups.length > 0 ? (
        <List>
          {groups.map((g) => (
            <AreaRows
              key={g.name}
              group={g}
              start={start}
              live={live}
              startable={board.areas.includes(g)}
            />
          ))}
        </List>
      ) : (
        <p className="text-label-13 text-gray-700">Nothing on this board yet.</p>
      )}
    </div>
  );
}

// The cards a project's hands can belong to: its live ones and those in its merge queue.
function cardIds(boards: BoardView[]): Set<string> {
  const ids = new Set<string>();
  for (const b of boards) {
    for (const r of b.live.cards) ids.add(r.id);
    if (b.merge.slot !== null) ids.add(b.merge.slot[0].id);
    for (const [r] of b.merge.waiting) ids.add(r.id);
  }
  return ids;
}

/** The workspace at `/`: figures strip, raised hands, active cards, merge queues, and project backlogs. */
export function Board({ screen, views, burn, nowMs, live }: { screen: Screen; views: Record<string, PlaceView>; burn: RecentBurn | null; nowMs: number; live: boolean }) {
  const [chosen, setChosen] = useState(ALL);
  const filter = screen.boards.some((b) => b.project === chosen) ? chosen : ALL;
  const boards = filter === ALL ? screen.boards : screen.boards.filter((b) => b.project === filter);
  const ids = cardIds(boards);
  const waiting = ownHands(screen.hands);
  const hands = filter === ALL ? waiting : waiting.filter((h) => h.card === null || ids.has(h.card));
  const queues = boards.filter((b) => b.merge.slot !== null || b.merge.in_line > 0);
  const [starting, setStarting] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader
        crumbs={<span>Workspace</span>}
        title="Board"
        right={
          screen.boards.length > 0 && (
            <>
              <Segmented
                options={[ALL, ...screen.boards.map((b) => b.project)]}
                value={filter}
                onChange={(v) => { setChosen(v); setStarting(null); }}
                label={(key) => (key === ALL ? "All projects" : key)}
              />
              <Button tier="primary" size="md" disabled={!live} pressed={creating} onClick={() => setCreating((v) => !v)}>
                New card
              </Button>
            </>
          )
        }
      />
      <PageScroll>
        {creating && (
          <NewCard
            live={live}
            projects={screen.boards.map((b) => b.project)}
            project={filter === ALL ? (screen.boards[0]?.project ?? "") : filter}
            onDone={() => setCreating(false)}
          />
        )}
        <KpiStrip items={kpis(screen, boards, hands, burn, nowMs, filter !== ALL)} />
        {hands.length > 0 && <RaisedHands hands={hands} nowMs={nowMs} live={live} />}
        <ActiveCards boards={boards} screen={screen} />
        {queues.length > 0 && (
          <section className="mt-gutter">
            <SectionHead title="Merge Queue" />
            {queues.map((b) => (
              <MergeQueueList key={b.project} project={b.project} queue={b.merge} nowMs={nowMs} />
            ))}
          </section>
        )}
        <section className="mt-gutter">
          <SectionHead title="Project Backlogs" right={<span className="text-label-12 text-gray-700">Areas sorted alphabetically</span>} />
          {boards.map((b) => (
            <ProjectBacklog
              key={b.project}
              board={b}
              start={{ id: starting, select: setStarting, view: starting ? views[starting] : undefined }}
              lookback={screen.projects?.find((p) => p.project === b.project)}
              nowMs={nowMs}
              live={live}
            />
          ))}
        </section>
      </PageScroll>
    </main>
  );
}
