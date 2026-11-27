import { useState } from "react";
import { NewCard, StartStrip } from "./BoardForms";
import { cardTarget, duration, plural, shortAge } from "./format";
import { ChevronIcon } from "./icons";
import { RefusalLine, useHandOffers } from "./offers";
import { PageLink, cardPath } from "./route";
import type { BoardGroup, BoardRow, BoardView, LookbackRead, MergeQueue, PlaceView, ProjectView, ReadView } from "./types";
import { Button } from "./ui/Button";
import { List, Row } from "./ui/List";
import { PageHeader, PageScroll } from "./ui/Page";
import { SectionHead } from "./ui/Section";
import { Segmented } from "./ui/Segmented";
const ID = "min-w-[76px] max-w-[220px] shrink-0 truncate font-mono text-label-13 font-medium text-gray-1000";
const MONO = "font-mono text-label-12 tabular-nums";
const READ_LABEL: Record<LookbackRead, string> = { dream: "Dream", pulse: "Pulse", "deep-sweep": "Deep sweep" };

function MergeQueueSection({ queue, nowMs }: { queue: MergeQueue; nowMs: number }) {
  const place = `w-[76px] shrink-0 whitespace-nowrap ${MONO}`;
  return (
    <section>
      <SectionHead
        title="Merge Queue"
        note={`→ ${queue.base}`}
        right={<span className={`${MONO} text-gray-700`}>{queue.slot !== null ? "1 in slot" : "slot free"} · {queue.in_line} in line</span>}
      />
      <List>
        {queue.slot !== null ? (
          <Row to={cardPath(cardTarget(queue.slot[0]))}>
            <span className={`${place} text-gray-1000`}>In slot</span>
            <span title={queue.slot[0].id} className={ID}>{queue.slot[0].id}</span>
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
            <span title={r.id} className={ID}>{r.id}</span>
            <span className="min-w-0 flex-1 truncate text-label-14 text-gray-900">{r.title}</span>
            <span className={`shrink-0 ${MONO} text-gray-900`}>waiting {duration(nowMs - at)}</span>
          </Row>
        ))}
      </List>
    </section>
  );
}

function lastRunText(row: ReadView, nowMs: number): string {
  if (row.last === null) return "none yet";
  const age = shortAge(nowMs - row.last.ended_ms);
  if (row.last.commits === null) return age;
  return `${age}, ${row.last.commits} ${plural(row.last.commits, "commit", "commits")} since`;
}

function LookBack({ project, rows, nowMs, live }: { project: string; rows: ReadView[]; nowMs: number; live: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const running = rows.some((r) => r.running !== null);
  return (
    <section className="mt-gutter">
      <SectionHead title="Look back" />
      <RefusalLine refusal={refusals[project] ?? null} className="mb-2" />
      <List>
        {rows.map((r) => {
          const label = READ_LABEL[r.read];
          return (
            <Row
              key={r.read}
              actions={
                r.running !== null ? (
                  <PageLink to={cardPath(r.running)} className="font-mono text-label-12 text-gray-1000 hover:underline">{r.running}</PageLink>
                ) : running ? undefined : (
                  <Button tier="secondary" size="sm" disabled={!live || pending[project] !== undefined} title={`Look back: ${project} ${label.toLowerCase()}`}
                    onClick={() => press(project, { label, command: { lookback: { project, read: r.read } } })}>
                    {pending[project] === label ? `${label}…` : label}
                  </Button>
                )
              }
            >
              <span className="w-[112px] shrink-0 text-label-14 text-gray-1000">{label}</span>
              <span className={`w-[88px] shrink-0 ${MONO} text-gray-700`}>{r.mode === "manual" ? "" : r.mode}</span>
              <span className="min-w-0 flex-1 truncate text-label-12 text-gray-700">{r.running !== null ? "running" : `last run ${lastRunText(r, nowMs)}`}</span>
            </Row>
          );
        })}
      </List>
    </section>
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

export function Project({ board, lookback, views, nowMs, live }: {
  board: BoardView;
  lookback: ProjectView | undefined;
  views: Record<string, PlaceView>;
  nowMs: number;
  live: boolean;
}) {
  const [creating, setCreating] = useState(false);
  const { pending, refusals, press } = useHandOffers();
  const [starting, setStarting] = useState<string | null>(null);
  const backlog = board.areas.reduce((n, g) => n + g.count, 0);
  const groups = [...board.areas, board.done].filter((g) => g.count > 0);
  const start = { id: starting, select: setStarting, view: starting ? views[starting] : undefined };
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader
        crumbs={<span>Project</span>}
        title={board.project}
        right={
          <>
            <span className="flex items-center gap-2">
              <span className="text-label-12 text-gray-700">Auto delegation</span>
              <Segmented
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
            <Button tier="primary" size="md" disabled={!live} pressed={creating} onClick={() => setCreating((v) => !v)}>New card</Button>
          </>
        }
      />
      <PageScroll>
        <RefusalLine refusal={refusals[board.project] ?? null} className="mb-2" />
        {creating && <NewCard live={live} project={board.project} onDone={() => setCreating(false)} />}
        <MergeQueueSection queue={board.merge} nowMs={nowMs} />
        {lookback !== undefined && lookback.rows.length > 0 && <LookBack project={board.project} rows={lookback.rows} nowMs={nowMs} live={live} />}
        <section className="mt-gutter">
          <SectionHead
            title="Backlog"
            note={`${backlog} backlog · ${board.done.count} ${board.done.name.toLowerCase()}`}
            right={<span className="text-label-12 text-gray-700">Areas sorted alphabetically</span>}
          />
          {groups.length > 0 ? (
            <List>
              {groups.map((g) => (
                <AreaRows key={g.name} group={g} start={start} live={live} startable={board.areas.includes(g)} />
              ))}
            </List>
          ) : (
            <p className="text-label-13 text-gray-700">Nothing on this board yet.</p>
          )}
        </section>
      </PageScroll>
    </main>
  );
}
