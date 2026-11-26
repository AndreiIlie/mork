import { useState } from "react";
import { countdown, dollars, plural } from "./format";
import { RefusalLine, useHandOffers } from "./offers";
import { quotaReading } from "./quota";
import type { DayBurn, RecentBurn, Screen, Share } from "./types";
import { Button } from "./ui/Button";
import { ChartLayout } from "./ui/ChartLayout";
import { type Kpi, KpiStrip } from "./ui/Kpi";
import { List, Row } from "./ui/List";
import { PageHeader, PageScroll } from "./ui/Page";
import { ProgressBar } from "./ui/ProgressBar";
import { SectionHead } from "./ui/Section";

const MONO = "font-mono text-label-12 tabular-nums";
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function money(cents: number, unknown: boolean): string {
  return `${dollars(cents)}${unknown ? " + ?" : ""}`;
}

function dayLabel(d: DayBurn, tzOffsetMin: number): string {
  const at = new Date(d.day_ms + tzOffsetMin * 60_000);
  return `${MONTHS[at.getUTCMonth()]} ${at.getUTCDate()}`;
}

function shortDayLabel(d: DayBurn, tzOffsetMin: number): string {
  const at = new Date(d.day_ms + tzOffsetMin * 60_000);
  const day = at.getUTCDate();
  return day === 1 ? `${at.getUTCMonth() + 1}/1` : String(day);
}

function kpis(burn: RecentBurn | null, shares: Share[], nowMs: number): Kpi[] {
  const left = burn?.cards_left;
  const pool = left === undefined ? undefined : shares.find((s) => s.cli === left.pool && s.window === "week");
  const resets = pool?.reset_ms != null && pool.reset_ms > nowMs ? ` · resets in ${countdown(pool.reset_ms - nowMs)}` : "";
  return [
    { label: "7-Day Spend", value: burn === null ? "?" : money(burn.total_cents, burn.unknown), sub: "last 7 days, every seat" },
    {
      label: "Average / Card",
      value: burn?.average_cents !== undefined ? dollars(burn.average_cents) : "?",
      sub: burn === null ? "" : `over ${burn.ended_cards} ended ${plural(burn.ended_cards, "card", "cards")}`,
    },
    {
      label: "Cards Left",
      value: left !== undefined ? `~${left.cards}` : "?",
      sub: left !== undefined ? `on ${left.pool} week${resets}` : "no weekly pool read yet",
    },
  ];
}

function SpendChart({ days, tzOffsetMin }: { days: DayBurn[]; tzOffsetMin: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = days[hover ?? days.length - 1];
  const max = Math.max(1, ...days.map((d) => d.cents));
  const top = Math.ceil(max / 1000) * 1000;
  const labels = days.map((d) => ({ full: dayLabel(d, tzOffsetMin), short: shortDayLabel(d, tzOffsetMin) }));

  return (
    <section className="mt-gutter border border-gray-400 px-edge py-3">
      <div className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1">
        <h2 className="text-heading-14 text-gray-1000">Daily Spend & Landed Cards</h2>
        <span className="text-label-12 text-gray-700">Last 14 days · bars are dollars, numbers are cards landed</span>
        {shown !== undefined && (
          <span className={`ml-auto ${MONO} text-gray-1000`}>
            {dayLabel(shown, tzOffsetMin)}: {money(shown.cents, shown.unknown)} · {shown.landed} landed
          </span>
        )}
      </div>
      <ChartLayout yLabels={[dollars(top), "$0"]} xLabels={labels} height={72} headroom xAlign="columns">
        <div className="flex h-full items-end" onMouseLeave={() => setHover(null)}>
          {days.map((d, i) => {
            const on = hover === null ? i === days.length - 1 : i === hover;
            return (
              <div
                key={d.day_ms}
                className="flex h-full min-w-0 flex-1 cursor-default flex-col items-center justify-end"
                onMouseEnter={() => setHover(i)}
              >
                <span className={`shrink-0 ${on ? "text-gray-1000" : "text-gray-700"}`}>{d.landed}</span>
                {d.cents > 0 && (
                  <div
                    className={`w-[64%] shrink-0 ${on ? "bg-gray-1000" : "bg-gray-500"}`}
                    style={{ height: `max(2px, ${(d.cents * 100) / top}%)` }}
                  />
                )}
              </div>
            );
          })}
        </div>
      </ChartLayout>
    </section>
  );
}

function PromptCacheHit({ burn }: { burn: RecentBurn | null }) {
  return (
    <section className="mt-gutter">
      <SectionHead title="Prompt Cache Hit" right={<span className="text-label-12 text-gray-700">Last 7 days</span>} />
      {burn === null || burn.cache_by_model.length === 0 ? (
        <p className="text-label-13 text-gray-700">{burn === null ? "Loading…" : "?"}</p>
      ) : (
        <List>
          {burn.cache_by_model.map(({ cli, model, cache_hit_percent }) => (
            <Row key={`${cli}\0${model}`}>
              <span className="shrink-0 text-gray-700">{cli}</span>
              <span className="min-w-0 flex-1 break-all">{model}</span>
              <span className={MONO}>{cache_hit_percent === undefined ? "?" : `${cache_hit_percent}%`}</span>
            </Row>
          ))}
        </List>
      )}
    </section>
  );
}

interface PoolGroup {
  key: string;
  name: string;
  readAgo: string | null;
  switchAccount: string | null;
  signedOut: boolean;
  windows: Share[];
}

function groupPools(shares: Share[]): PoolGroup[] {
  const groups: PoolGroup[] = [];
  for (const share of shares) {
    const key = `${share.cli}\0${share.signed_out === true ? "1" : "0"}\0${share.switch_account ?? ""}`;
    const last = groups[groups.length - 1];
    if (last !== undefined && last.key === key) {
      last.windows.push(share);
    } else {
      groups.push({ key, name: share.cli, readAgo: null, switchAccount: null, signedOut: true, windows: [share] });
    }
  }
  for (const g of groups) {
    g.readAgo = g.windows.find((w) => w.read_ago != null)?.read_ago ?? null;
    g.switchAccount = g.windows.find((w) => w.switch_account !== undefined)?.switch_account ?? null;
    g.signedOut = g.windows.every((w) => w.signed_out === true);
  }
  return groups;
}

function WindowRow({ w, signedOut, nowMs }: { w: Share; signedOut: boolean; nowMs: number }) {
  const q = quotaReading(w, nowMs, signedOut);
  return (
    <Row indent={1}>
      <span className={`w-12 shrink-0 ${MONO} text-gray-700`}>{w.window}</span>
      <span className="min-w-0 flex-1">
        <ProgressBar percent={q.barPercent} tone={q.tone} tick={q.tick} height={6} />
      </span>
      <span className={`w-[60px] shrink-0 text-right ${MONO} ${q.hot ? "text-amber-700" : signedOut || q.stale ? "text-gray-900" : "text-gray-1000"}`}>
        {q.words}
      </span>
      <span className={`w-[52px] shrink-0 text-right ${MONO} ${q.hot ? "text-amber-700" : "text-gray-700"}`}>
        {q.ends !== null ? `→${q.ends}%` : ""}
      </span>
      <span className={`hidden w-[136px] shrink-0 text-right sm:block ${MONO} text-gray-700`}>
        {q.resetLeft !== null ? `resets in ${q.resetLeft}` : ""}
      </span>
    </Row>
  );
}

function AccountQuotas({ shares, nowMs, live }: { shares: Share[]; nowMs: number; live: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const pools = groupPools(shares);
  const busy = Object.keys(pending).length > 0;

  return (
    <div className="min-w-0">
      <SectionHead title="Account Quotas" right={<span className="text-label-12 text-gray-700">tick = window elapsed</span>} />
      {pools.length === 0 ? (
        <p className="text-label-13 text-gray-700">No usage pools reported yet.</p>
      ) : (
        <List>
          {pools.map((p) => {
            const account = p.switchAccount;
            return (
              <div key={p.key} className="divide-y divide-gray-400">
                <Row
                  actions={
                    account !== null ? (
                      <Button
                        tier="secondary"
                        size="sm"
                        disabled={!live || busy}
                        onClick={() => press(p.key, { label: "Switch", command: { "claude.account": { name: account } } })}
                      >
                        {pending[p.key] !== undefined ? "Switching…" : "Switch"}
                      </Button>
                    ) : undefined
                  }
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-3">
                      <span className={`text-label-14 ${p.signedOut ? "text-gray-900" : "text-gray-1000"}`}>{p.name}</span>
                      {p.signedOut && <span className="text-label-12 text-gray-700">signed out</span>}
                      {p.readAgo !== null && <span className="text-label-12 text-gray-700">read {p.readAgo} ago</span>}
                    </div>
                    <RefusalLine refusal={refusals[p.key] ?? null} className="mt-1" />
                  </div>
                </Row>
                {p.windows.map((w) => (
                  <WindowRow key={w.window} w={w} signedOut={p.signedOut} nowMs={nowMs} />
                ))}
              </div>
            );
          })}
        </List>
      )}
    </div>
  );
}

function BurnByRole({ burn }: { burn: RecentBurn | null }) {
  return (
    <div className="min-w-0">
      <SectionHead title="Burn by Role" right={<span className="text-label-12 text-gray-700">Last 7 days</span>} />
      {burn === null ? (
        <p className="text-label-13 text-gray-700">Loading…</p>
      ) : burn.roles.length === 0 ? (
        <p className="text-label-13 text-gray-700">No burn recorded in the last 7 days.</p>
      ) : (
        <List>
          {burn.roles.map((r, i) => {
            const share = burn.total_cents > 0 ? Math.round((r.cents * 100) / burn.total_cents) : 0;
            return (
              <Row key={r.role}>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-3">
                    <span className="text-label-14 text-gray-1000">{r.role}</span>
                    <span className={`ml-auto ${MONO} text-gray-1000`}>{money(r.cents, r.unknown)}</span>
                    <span className={`w-9 text-right ${MONO} text-gray-700`}>{share}%</span>
                  </span>
                  <span className="mt-2 block">
                    <ProgressBar percent={share} tone={i === 0 ? "strong" : "neutral"} />
                  </span>
                </span>
              </Row>
            );
          })}
        </List>
      )}
    </div>
  );
}

/** The Usage page at `/usage`: weekly figures, per-model cache rates, daily spend, quotas, and burn by role. */
export function Usage({
  screen,
  burn,
  burnError,
  nowMs,
  live,
}: {
  screen: Screen;
  burn: RecentBurn | null;
  burnError: string | null;
  nowMs: number;
  live: boolean;
}) {
  const days = burn?.daily ?? [];
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader crumbs={<span>Workspace</span>} title="Usage" />
      <PageScroll>
        <RefusalLine refusal={burnError} className="mb-3" />
        <KpiStrip items={kpis(burn, screen.top.usage, nowMs)} />
        <PromptCacheHit burn={burn} />
        {days.length > 0 && <SpendChart days={days} tzOffsetMin={screen.tz_offset_min} />}
        <div className="mt-gutter grid grid-cols-1 gap-gutter xl:grid-cols-2">
          <AccountQuotas shares={screen.top.usage} nowMs={nowMs} live={live} />
          <BurnByRole burn={burn} />
        </div>
      </PageScroll>
    </main>
  );
}
