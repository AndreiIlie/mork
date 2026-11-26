import { useState } from "react";
import { type AlertPermission, ownHands } from "./alerts";
import type { Link } from "./bridge";
import { ago, cardTarget, coldMinutes, dollars, wherePlace, workingSeats } from "./format";
import { BoardIcon, ChevronIcon, Cold, HandIcon, MorkMark } from "./icons";
import { RefusalLine, useHandOffers } from "./offers";
import { quotaReading } from "./quota";
import { PageLink, cardPath } from "./route";
import type { BoardView, HandView, RecentBurn, Screen, Share } from "./types";
import { Button } from "./ui/Button";
import { Dot, StatusDot } from "./ui/Dot";
import { ProgressBar } from "./ui/ProgressBar";
import { TextLink } from "./ui/TextLink";

const LINK_WORDS: Record<Link, string> = {
  live: "Connected",
  connecting: "Connecting…",
  reconnecting: "Reconnecting…",
  disconnected: "Disconnected",
};

const LINK_DOT = { live: "green", connecting: "yellow", reconnecting: "yellow", disconnected: "grey" } as const;

const ROW = "block px-edge py-2 transition-colors";
const LABEL = "text-label-12 text-gray-700";
const HOVER = "hover:bg-gray-100";
const ACTIVE = "bg-gray-200";

const WINDOW_WORDS: Record<string, string> = { "5h": "5h", week: "wk" };

function HandItem({ hand, activeCard, nowMs }: { hand: HandView; activeCard: string | null; nowMs: number }) {
  const cold = coldMinutes(hand.cold_at_ms, nowMs);
  const active = hand.card !== null && activeCard === hand.card;
  const body = (
    <>
      <div className="flex items-center gap-2">
        <Dot color={hand.color} />
        <span title={hand.card ?? hand.place} className="min-w-0 truncate font-mono text-label-12 font-medium text-gray-1000">
          {hand.card ?? hand.place}
        </span>
        <span className="min-w-0 shrink-[4] truncate text-label-12 text-gray-900">
          {[wherePlace(hand), hand.role ?? (hand.card === null ? null : "engine"), hand.design && "design"].filter(Boolean).join(" · ")}
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-2">
          {hand.card !== null && <span className={LABEL}>{ago(nowMs - hand.raised_ms)}</span>}
          {cold !== null && <Cold minutes={cold} />}
        </span>
      </div>
      <p className="mt-0.5 truncate pl-4 text-label-12 text-gray-900">{hand.card === null ? hand.words : hand.headline}</p>
    </>
  );
  const className = `${ROW} ${active ? ACTIVE : HOVER}`;
  return (
    <li>
      {hand.card !== null ? (
        <PageLink to={cardPath(hand.card, hand.ask)} className={className}>
          {body}
        </PageLink>
      ) : (
        <div className={className}>{body}</div>
      )}
    </li>
  );
}

function ProjectTree({
  board,
  screen,
  activeCard,
  open,
  onToggle,
}: {
  board: BoardView;
  screen: Screen;
  activeCard: string | null;
  open: boolean;
  onToggle: () => void;
}) {
  const waiting = ownHands(screen.hands);
  return (
    <div className="border-b border-gray-400">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className={`flex h-7 w-full cursor-pointer items-center gap-2 px-edge text-left ${LABEL} hover:text-gray-1000`}
      >
        <ChevronIcon open={open} />
        <span className="truncate">{board.project}</span>
        <span className="ml-auto font-mono tabular-nums">{board.live.count}</span>
      </button>
      {open && (
        <ul>
          {board.live.cards.map((r) => {
            const target = cardTarget(r);
            const raised = waiting.some((h) => h.card === r.id);
            const working = workingSeats(r, screen.rooms).length > 0;
            return (
              <li key={r.id}>
                <PageLink to={cardPath(target)} className={`${ROW} ${activeCard === target ? ACTIVE : HOVER}`}>
                  <div className="flex items-center gap-2">
                    <StatusDot light={r.live?.light ?? null} working={working} />
                    <span title={r.id} className="min-w-0 truncate font-mono text-label-12 font-medium text-gray-1000">
                      {r.id}
                    </span>
                    {raised && <HandIcon className="h-3 w-3 text-gray-1000" />}
                    <span className="ml-auto shrink-0 font-mono text-label-12 text-gray-700">{r.live?.stage}</span>
                  </div>
                  <div className="mt-0.5 truncate pl-4 text-label-12 text-gray-900">{r.title}</div>
                </PageLink>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function UsageNav({ active }: { active: boolean }) {
  return (
    <PageLink
      to="/usage"
      className={`flex h-9 shrink-0 items-center gap-2 px-edge text-label-14 transition-colors ${
        active ? "bg-gray-200 font-medium text-gray-1000" : "text-gray-900 hover:bg-gray-100 hover:text-gray-1000"
      }`}
    >
      <span>Usage</span>
    </PageLink>
  );
}

function BoardNav({ screen, onBoard, onUsage }: { screen: Screen | null; onBoard: boolean; onUsage: boolean }) {
  const totalLive = screen?.boards.reduce((n, b) => n + b.live.count, 0) ?? 0;
  return (
    <nav className="border-b border-gray-400">
      <PageLink
        to="/"
        className={`flex h-9 items-center gap-2 px-edge text-label-14 transition-colors ${
          onBoard ? "bg-gray-200 font-medium text-gray-1000" : "text-gray-900 hover:bg-gray-100 hover:text-gray-1000"
        }`}
      >
        <BoardIcon />
        <span>Board</span>
        {screen !== null && <span className="ml-auto font-mono text-label-12 tabular-nums text-gray-700">{totalLive}</span>}
      </PageLink>
      <UsageNav active={onUsage} />
    </nav>
  );
}

function HandsRail({ hands, activeCard, nowMs }: { hands: HandView[]; activeCard: string | null; nowMs: number }) {
  return (
    <div className="border-b border-gray-400">
      <div className={`flex h-7 items-center px-edge ${LABEL}`}>
        <span>Raised Hands</span>
        <span className="ml-auto font-mono tabular-nums">{hands.length}</span>
      </div>
      <ul>
        {hands.map((h) => (
          <HandItem key={h.ask} hand={h} activeCard={activeCard} nowMs={nowMs} />
        ))}
      </ul>
    </div>
  );
}

function UpdateReadyRow({ pin, live, reloading }: { pin: string | null; live: boolean; reloading: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const busy = pending.reload !== undefined;

  return (
    <div className="shrink-0 border-b border-gray-400 px-edge py-2">
      <div className="flex items-center gap-2">
        <span className="text-label-12 text-gray-1000">Update ready</span>
        {pin !== null && <span className="font-mono text-label-12 text-gray-700">{pin}</span>}
        <span className="ml-auto shrink-0">
          <Button
            tier="secondary"
            size="sm"
            disabled={!live || busy || reloading}
            onClick={() => press("reload", { label: "Reload", command: { reload: {} } })}
          >
            {busy ? "Reloading…" : "Reload"}
          </Button>
        </span>
      </div>
      <RefusalLine refusal={refusals.reload ?? null} className="mt-1" />
    </div>
  );
}

// One Claude account is signed in at a time; agy's pools differ by their bucket.
function poolLabel(cli: string): string {
  const words = cli.split(" ");
  const name = words[0] === "claude" ? "claude" : (words[words.length - 1] ?? cli);
  return `${name.charAt(0).toUpperCase()}${name.slice(1)}`;
}

function spendLabel(burn: RecentBurn | null): string {
  if (burn === null) return "? / 7d";
  return `${dollars(burn.total_cents)}${burn.unknown ? " + ?" : ""} / 7d`;
}

// 5h and week of signed-in accounts only; fable and signed-out accounts stay on the Usage page.
function Quotas({ shares, burn, nowMs }: { shares: Share[]; burn: RecentBurn | null; nowMs: number }) {
  const rows = shares.filter((w) => w.signed_out !== true && WINDOW_WORDS[w.window] !== undefined);
  return (
    <PageLink to="/usage" className="block shrink-0 border-t border-gray-400 px-edge py-3 transition-colors hover:bg-gray-100">
      <div className="flex items-center justify-between text-label-12 text-gray-900">
        <span>Quotas</span>
        <span className="font-mono tabular-nums text-gray-1000">{spendLabel(burn)}</span>
      </div>
      {rows.map((w) => {
        const label = `${poolLabel(w.cli)} ${WINDOW_WORDS[w.window]}`;
        const q = quotaReading(w, nowMs, false);
        return (
          <div
            key={`${w.cli} ${w.window}`}
            className="mt-1.5 grid grid-cols-[72px_1fr_52px] items-center gap-2 font-mono text-label-12 tabular-nums text-gray-700"
          >
            <span className="truncate">{label}</span>
            <ProgressBar percent={q.barPercent} tone={q.tone} tick={q.tick} />
            <span className={`text-right ${q.hot ? "text-amber-700" : "text-gray-900"}`}>{q.words}</span>
          </div>
        );
      })}
    </PageLink>
  );
}

export function Sidebar({
  screen,
  burn = null,
  link,
  build,
  activeCard,
  onUsage = false,
  nowMs,
  notifications,
}: {
  screen: Screen | null;
  burn?: RecentBurn | null;
  link: Link;
  build: string | null;
  activeCard: string | null;
  onUsage?: boolean;
  nowMs: number;
  notifications?: AlertPermission;
}) {
  const [closed, setClosed] = useState<Record<string, boolean>>({});
  const isOpen = (project: string) => !(closed[project] ?? false);
  const toggleProject = (project: string) => setClosed((prev) => ({ ...prev, [project]: !prev[project] }));
  // Drop hands represented by a currently rendered card row; collapsing a project restores its cards' asks here.
  const rendered = new Set(
    screen?.boards.filter((b) => isOpen(b.project)).flatMap((b) => b.live.cards.map((r) => r.id)) ?? [],
  );
  const alerts = ownHands(screen?.hands ?? []).filter((h) => h.card === null || !rendered.has(h.card));
  const title = build === null ? LINK_WORDS[link] : `${LINK_WORDS[link]} · build ${build}`;
  const updateReady = screen?.top.pin?.ready === true;
  const reloading = screen?.top.phase.is === "reloading";
  return (
    <aside className="flex w-[260px] shrink-0 flex-col border-r border-gray-400 bg-background-100">
      <div className="flex h-12 shrink-0 items-center gap-[10px] border-b border-gray-400 px-edge">
        <MorkMark />
        <span className="text-heading-14 text-gray-1000">Mork</span>
        <span title={title} className="ml-auto font-mono text-label-12 text-gray-700">
          {build === null ? "offline" : build.slice(0, 8)}
        </span>
      </div>

      {updateReady && <UpdateReadyRow pin={screen?.top.pin.pin ?? null} live={link === "live"} reloading={reloading} />}

      <div className="min-h-0 flex-1 overflow-y-auto">
        <BoardNav screen={screen} onBoard={activeCard === null && !onUsage} onUsage={onUsage} />
        {alerts.length > 0 && <HandsRail hands={alerts} activeCard={activeCard} nowMs={nowMs} />}
        {screen?.boards.map((b) => (
          <ProjectTree
            key={b.project}
            board={b}
            screen={screen}
            activeCard={activeCard}
            open={isOpen(b.project)}
            onToggle={() => toggleProject(b.project)}
          />
        ))}
      </div>

      {screen !== null && <Quotas shares={screen.top.usage} burn={burn} nowMs={nowMs} />}

      <div className="flex h-9 shrink-0 items-center gap-2 border-t border-gray-400 px-edge text-label-12 text-gray-900">
        <Dot color={LINK_DOT[link]} />
        <span className="truncate">{LINK_WORDS[link]}</span>
        {notifications?.canAsk && (
          <span className="ml-auto shrink-0">
            <TextLink onClick={notifications.request}>Enable alerts</TextLink>
          </span>
        )}
      </div>
    </aside>
  );
}
