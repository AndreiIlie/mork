import { ReloadChangelog } from "./ReloadChangelog";
import type { Link } from "./bridge";
import { useHandOffers, RefusalLine } from "./offers";
import { useState } from "react";
import { type AlertPermission, ownHands } from "./alerts";
import { cardTarget, workingSeats } from "./format";
import { BoardIcon, ChevronIcon, HandIcon, MorkMark } from "./icons";
import { hottest } from "./quota";
import { PageLink, cardPath } from "./route";
import type { BoardView, PlaceView, Screen } from "./types";
import { Button } from "./ui/Button";
import { Dot, StatusDot } from "./ui/Dot";

const LINK_WORDS: Record<Link, string> = {
  live: "Connected",
  connecting: "Connecting…",
  reconnecting: "Reconnecting…",
  disconnected: "Disconnected",
};

const LINK_DOT = { live: "green", connecting: "yellow", reconnecting: "yellow", disconnected: "grey" } as const;

const ROW = "block px-edge py-2 transition-colors";
const HOVER = "hover:bg-gray-100";
const ACTIVE = "bg-gray-200";
const NAV = "flex h-9 items-center gap-2 px-edge text-label-14 transition-colors";
const NAV_ON = "bg-gray-200 font-medium text-gray-1000";
const NAV_OFF = "text-gray-900 hover:bg-gray-100 hover:text-gray-1000";
function ProjectTree({ board, screen, activeCard }: { board: BoardView; screen: Screen; activeCard: string | null }) {
  const [open, setOpen] = useState(true);
  const waiting = ownHands(screen.hands);
  return (
    <div className="border-b border-gray-400">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex h-8 w-full cursor-pointer items-center gap-2 px-edge text-left text-label-12 text-gray-900 hover:text-gray-1000"
      >
        <ChevronIcon open={open} />
        <span className="truncate font-medium">{board.project}</span>
        <span className="ml-auto font-mono tabular-nums text-gray-700">{board.live.count}</span>
      </button>
      {open && (
        <ul>
          {board.live.cards.map((r) => {
            const target = cardTarget(r);
            const raised = waiting.some((h) => h.card === r.id);
            const working = workingSeats(r, screen.rooms).length > 0;
            const active = activeCard === target;
            return (
              <li key={r.id}>
                <PageLink to={cardPath(target)} className={`${ROW} ${active ? ACTIVE : HOVER}`}>
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

export function Sidebar({
  screen,
  link,
  notifications,
  build,
  mainView,
  activeCard,
  onUsage,
  nowMs,
}: {
  screen: Screen | null;
  link: Link;
  notifications: AlertPermission;
  build: string | null;
  mainView: PlaceView | undefined;
  activeCard: string | null;
  onUsage: boolean;
  nowMs: number;
}) {
  const hands = ownHands(screen?.hands ?? []);
  const onBoard = activeCard === null && !onUsage;
  const quota = hottest(screen?.top.usage ?? [], nowMs);
  const resources = screen?.top.resources;
  const updateReady = screen?.top.pin?.ready;
  const { pending, refusals, press } = useHandOffers();
  const busy = pending.reload !== undefined || screen?.top.phase.is === "reloading";
  return (
    <aside className="flex w-[260px] shrink-0 flex-col border-r border-gray-400 bg-background-100">
      <div className="flex h-12 shrink-0 items-center gap-[10px] border-b border-gray-400 px-edge">
        <MorkMark />
        <span className="text-heading-14 text-gray-1000">Mork</span>
        {build === null ? (
          <span className="ml-auto font-mono text-label-12 text-gray-700">offline</span>
        ) : (
          <ReloadChangelog build={build} view={mainView} live={link === "live"} tzOffsetMin={screen?.tz_offset_min ?? 0} />
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <nav className="border-b border-gray-400">
          <PageLink to="/" className={`${NAV} ${onBoard ? NAV_ON : NAV_OFF}`}>
            <BoardIcon />
            <span>Board</span>
            {hands.length > 0 && (
              <span title={`${hands.length} raised hands`} className="ml-auto flex items-center gap-1.5 text-gray-1000">
                <HandIcon className="h-3 w-3" />
                <span className="font-mono text-label-12 font-medium tabular-nums">{hands.length}</span>
              </span>
            )}
          </PageLink>
          <PageLink to="/usage" className={`${NAV} ${onUsage ? NAV_ON : NAV_OFF}`}>
            <span className="w-[14px]" aria-hidden="true" />
            <span>Usage</span>
          </PageLink>
        </nav>
        {screen?.boards.map((b) => (
          <ProjectTree key={b.project} board={b} screen={screen} activeCard={activeCard} />
        ))}
      </div>

      {notifications.canAsk && (
        <button
          type="button"
          onClick={notifications.request}
          className="flex h-control shrink-0 cursor-pointer items-center border-t border-gray-400 px-edge text-left text-label-12 text-gray-900 transition-colors hover:bg-gray-100 hover:text-gray-1000"
        >
          Enable alerts
        </button>
      )}

      {resources && (
        <div className="flex h-9 shrink-0 items-center justify-between gap-2 border-t border-gray-400 px-edge text-label-12 text-gray-700">
          <span>CPU <span className="font-mono tabular-nums text-gray-1000">
            {resources.cpu_percent === null ? "?" : `${resources.cpu_percent}%`}
          </span></span>
          <span>RAM <span className="font-mono tabular-nums text-gray-1000">
            {resources.ram === null ? "?" : `${Math.round(resources.ram.used_mib / 1024)}/${Math.round(resources.ram.total_mib / 1024)} GiB`}
          </span></span>
        </div>
      )}

      {updateReady && (
        <div className="flex h-9 shrink-0 items-center gap-2 border-t border-gray-400 px-edge text-label-12">
          <span className="text-gray-1000">Update ready</span>
          <span className="font-mono text-gray-700">{screen?.top.pin.pin}</span>
        </div>
      )}

      <RefusalLine refusal={refusals.reload ?? null} className="px-edge py-1" />
      <div className="flex h-11 shrink-0 items-center gap-2 border-t border-gray-400 pl-edge pr-2 text-label-12 text-gray-900">
        {link === "live" ? (
          quota !== null && (
            <PageLink to="/usage" className="flex min-w-0 items-center gap-2 font-mono tabular-nums hover:text-gray-1000" >
              <span className="text-gray-700">{quota.label}</span>
              <span className={quota.hot ? "text-amber-700" : "text-gray-1000"}>{quota.words}</span>
            </PageLink>
          )
        ) : (
          <span className="flex min-w-0 items-center gap-2">
            <Dot color={LINK_DOT[link] ?? "grey"} />
            <span className="truncate">{LINK_WORDS[link] ?? "Disconnected"}</span>
          </span>
        )}
        <span className="ml-auto shrink-0">
          <Button tier={updateReady ? "primary" : "secondary"} size="sm" disabled={link !== "live" || busy} onClick={() => press("reload", { label: "Reload", command: { reload: {} } })} title="Reload Mork to its pinned build">
            Reload
          </Button>
        </span>
      </div>
    </aside>
  );
}
