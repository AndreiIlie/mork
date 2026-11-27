import { useState } from "react";
import { ownHands } from "./alerts";
import type { Link } from "./bridge";
import { cardTarget, workingSeats } from "./format";
import { ChevronIcon, HandIcon, MorkMark, SettingsIcon } from "./icons";
import { closestQuotas } from "./quota";
import { type Route, PageLink, SETTINGS_PATH, cardPath, projectPath } from "./route";
import type { BoardView, Screen, Share } from "./types";
import { Dot, StatusDot } from "./ui/Dot";
import { ProgressBar } from "./ui/ProgressBar";

const LINK_WORDS: Record<Link, string> = { live: "Connected", connecting: "Connecting…", reconnecting: "Reconnecting…", disconnected: "Disconnected" };
const LINK_DOT = { live: "green", connecting: "yellow", reconnecting: "yellow", disconnected: "grey" } as const;
const ROW = "block px-edge py-2 transition-colors";

function Quotas({ shares, nowMs }: { shares: Share[]; nowMs: number }) {
  const rows = closestQuotas(shares, nowMs);
  if (rows.length === 0) return null;
  return (
    <PageLink to={SETTINGS_PATH.usage} className="block shrink-0 border-t border-gray-400 px-edge py-3 transition-colors hover:bg-gray-100">
      <span className="flex flex-col gap-2.5">
        {rows.map(({ label, reading: q }) => (
          <span key={label} className="flex flex-col gap-1.5">
            <span className="flex items-baseline gap-2 font-mono text-label-12 tabular-nums">
              <span className="min-w-0 truncate text-gray-900">{label}</span>
              <span className={`ml-auto shrink-0 ${q.hot ? "text-amber-700" : "text-gray-1000"}`}>{q.words}</span>
              <span className="w-[46px] shrink-0 text-right text-gray-700">{q.resetLeft ?? ""}</span>
            </span>
            <ProgressBar percent={q.barPercent} tone={q.tone} tick={q.tick} height={2} />
          </span>
        ))}
      </span>
    </PageLink>
  );
}

function ProjectTree({ board, screen, route }: { board: BoardView; screen: Screen; route: Route }) {
  const [open, setOpen] = useState(true);
  const waiting = ownHands(screen.hands);
  const here = route.page === "project" && route.project === board.project;
  return (
    <div className="border-b border-gray-400">
      <div className={`flex h-8 items-center text-label-12 transition-colors ${here ? "bg-gray-200" : "hover:bg-gray-100"}`}>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          aria-label={`${open ? "Collapse" : "Expand"} ${board.project}`}
          title={`${open ? "Collapse" : "Expand"} ${board.project}`}
          className="flex h-8 shrink-0 cursor-pointer items-center pl-edge pr-2 text-gray-900 hover:text-gray-1000"
        >
          <ChevronIcon open={open} />
        </button>
        <PageLink to={projectPath(board.project)} className={`flex h-8 min-w-0 flex-1 items-center gap-2 pr-edge ${here ? "text-gray-1000" : "text-gray-900 hover:text-gray-1000"}`}>
          <span className="truncate font-medium">{board.project}</span>
          <span className="ml-auto font-mono tabular-nums text-gray-700">{board.live.count}</span>
        </PageLink>
      </div>
      {open && (
        <ul>
          {board.live.cards.map((r) => {
            const target = cardTarget(r);
            const raised = waiting.some((h) => h.card === r.id);
            const working = workingSeats(r, screen.rooms).length > 0;
            const active = route.page === "card" && route.id === target;
            return (
              <li key={r.id}>
                <PageLink to={cardPath(target)} className={`${ROW} ${active ? "bg-gray-200" : "hover:bg-gray-100"}`}>
                  <div className="flex items-center gap-2">
                    <StatusDot light={r.live?.light ?? null} working={working} />
                    <span className="min-w-0 truncate font-mono text-label-12 font-medium text-gray-1000">{r.id}</span>
                    {raised && <HandIcon className="h-3 w-3 text-gray-1000" />}
                    <span className="ml-auto shrink-0 font-mono text-label-12 text-gray-700">{r.live?.stage}</span>
                  </div>
                  <div className={`mt-0.5 truncate pl-4 text-label-12 ${active ? "text-gray-1000" : "text-gray-900"}`}>{r.title}</div>
                </PageLink>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

export function Sidebar({ screen, link, build, route, nowMs, resourcesOn }: {
  screen: Screen | null;
  link: Link;
  build: string | null;
  route: Route;
  nowMs: number;
  resourcesOn: boolean;
}) {
  const hands = ownHands(screen?.hands ?? []);
  const resources = screen?.top.resources;
  const updateReady = screen?.top.pin?.ready;
  const onConsole = route.page === "console";
  const onSettings = route.page === "settings";
  return (
    <aside className="flex w-[260px] shrink-0 flex-col border-r border-gray-400 bg-background-100">
      <PageLink to="/" className={`flex h-12 shrink-0 items-center gap-[10px] border-b border-gray-400 px-edge transition-colors ${onConsole ? "bg-gray-200" : "hover:bg-gray-100"}`}>
        <MorkMark />
        <span className="text-heading-14 text-gray-1000">Mork</span>
        {hands.length > 0 && (
          <span title={`${hands.length} raised hands`} className="ml-auto flex items-center gap-1.5 text-gray-1000">
            <HandIcon className="h-3 w-3" />
            <span className="font-mono text-label-12 font-medium tabular-nums">{hands.length}</span>
          </span>
        )}
      </PageLink>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {screen?.boards.map((b) => (
          <ProjectTree key={b.project} board={b} screen={screen} route={route} />
        ))}
      </div>

      {resourcesOn && resources && (
        <div className="flex h-9 shrink-0 items-center justify-between gap-2 border-t border-gray-400 px-edge text-label-12 text-gray-700">
          <span>CPU <span className="font-mono tabular-nums text-gray-1000">{resources.cpu_percent === null ? "?" : `${resources.cpu_percent}%`}</span></span>
          <span>RAM <span className="font-mono tabular-nums text-gray-1000">
            {resources.ram === null ? "?" : `${Math.round(resources.ram.used_mib / 1024)}/${Math.round(resources.ram.total_mib / 1024)} GiB`}
          </span></span>
        </div>
      )}

      {link === "live" ? (
        <Quotas shares={screen?.top.usage ?? []} nowMs={nowMs} />
      ) : (
        <div className="flex h-9 shrink-0 items-center gap-2 border-t border-gray-400 px-edge text-label-12 text-gray-900">
          <Dot color={LINK_DOT[link]} />
          <span className="truncate">{LINK_WORDS[link]}</span>
        </div>
      )}

      <PageLink to={SETTINGS_PATH.general} className={`flex h-11 shrink-0 items-center gap-2 border-t border-gray-400 px-edge text-label-14 transition-colors ${onSettings ? "bg-gray-200 font-medium text-gray-1000" : "text-gray-900 hover:bg-gray-100 hover:text-gray-1000"}`}>
        <SettingsIcon />
        <span>Settings</span>
        {updateReady ? (
          <span className="ml-auto text-label-12 font-normal text-gray-1000">Update ready</span>
        ) : (
          <span className="ml-auto font-mono text-label-12 font-normal text-gray-700">{build === null ? "offline" : build.slice(0, 8)}</span>
        )}
      </PageLink>
    </aside>
  );
}
