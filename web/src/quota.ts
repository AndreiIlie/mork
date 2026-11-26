import { countdown } from "./format";
import type { Share } from "./types";
import type { ProgressTone } from "./ui/ProgressBar";

// Window lengths match crates/engine/src/cli/limits.rs (`5h`, `week`, `fable` = 7 days).
const WINDOW_MS: Record<string, number> = {
  "5h": 5 * 3_600_000,
  week: 7 * 86_400_000,
  fable: 7 * 86_400_000,
};

export interface QuotaReading {
  stale: boolean;
  reached: boolean;
  percent: number | null;
  ends: number | null;
  hot: boolean;
  words: string;
  barPercent: number;
  tone: ProgressTone;
  tick: number | null;
  resetLeft: string | null;
}

/** The share of the window already gone, from its reset time: live with the clock, right at 0% used. */
export function elapsedPercent(w: Share, nowMs: number): number | null {
  const length = WINDOW_MS[w.window];
  if (length === undefined || w.reset_ms == null) return null;
  const left = w.reset_ms - nowMs;
  return left > 0 && left < length ? ((length - left) / length) * 100 : null;
}

/** Shared pace, tone, tick, and reset reading for a quota window, used by Sidebar and Usage. */
export function quotaReading(w: Share, nowMs: number, signedOut = w.signed_out === true): QuotaReading {
  const stale = w.stale || (w.reset_ms != null && w.reset_ms <= nowMs);
  const reached = w.reached && !stale;
  const percent = stale || w.percent === null ? null : w.percent;
  const ends = stale ? null : (w.ends_percent ?? null);
  const hot = reached || (ends !== null && ends > 100);
  const words = stale ? "stale" : reached ? "reached" : percent !== null ? `${percent}%` : "?";
  const barPercent = reached ? 100 : (percent ?? 0);
  const tone: ProgressTone = hot ? "hot" : ends !== null && !signedOut ? "ok" : "neutral";
  const tick = stale ? null : elapsedPercent(w, nowMs);
  const resetLeft =
    !stale && w.window !== "fable" && w.reset_ms != null && w.reset_ms > nowMs ? countdown(w.reset_ms - nowMs) : null;
  return { stale, reached, percent, ends, hot, words, barPercent, tone, tick, resetLeft };
}

const WINDOW_WORDS: Record<string, string> = { "5h": "5h", week: "wk" };

function poolLabel(cli: string): string {
  const words = cli.split(" ");
  const name = words[0] === "claude" ? "claude" : (words[words.length - 1] ?? cli);
  return `${name.charAt(0).toUpperCase()}${name.slice(1)}`;
}

// The hottest window: an over-pace one first, then the highest percent used.
export function hottest(shares: Share[], nowMs: number): { label: string; words: string; hot: boolean } | null {
  const rows = shares
    .filter((w) => w.signed_out !== true && WINDOW_WORDS[w.window] !== undefined)
    .map((w) => ({ w, q: quotaReading(w, nowMs, false) }))
    .filter(({ q }) => q.words !== "?");
  if (rows.length === 0) return null;
  rows.sort((a, b) => Number(b.q.hot) - Number(a.q.hot) || (b.q.barPercent) - (a.q.barPercent));
  const { w, q } = rows[0];
  return { label: `${poolLabel(w.cli)} ${WINDOW_WORDS[w.window]}`, words: q.words, hot: q.hot };
}
