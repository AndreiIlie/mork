import type { Author, BoardRow, Color, HandView, RoomRow } from "./types";

// Show the last ten minutes and hide it once cold.
export function coldMinutes(coldAtMs: number | undefined, nowMs: number): number | null {
  if (coldAtMs === undefined || coldAtMs <= nowMs) return null;
  const m = Math.ceil((coldAtMs - nowMs) / 60_000);
  return m <= 10 ? m : null;
}

export function duration(ms: number): string {
  const m = Math.max(0, Math.floor(ms / 60_000));
  if (m < 1) return "just now";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem === 0 ? `${h}h` : `${h}h ${rem}m`;
}

export function ago(ms: number): string {
  const d = duration(ms);
  return d === "just now" ? d : `${d} ago`;
}

export function shortAge(ms: number): string {
  const mins = Math.max(0, Math.floor(ms / 60_000));
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function countdown(ms: number): string {
  const mins = Math.floor(Math.max(0, ms) / 60_000);
  if (mins < 1) return "<1m";
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;
  if (d > 0) return `${d}d${h}h${m}m`;
  if (h > 0) return `${h}h${m}m`;
  return `${m}m`;
}

export function dollars(cents: number): string {
  return `$${Math.floor(cents / 100)}.${String(cents % 100).padStart(2, "0")}`;
}

// Elapsed times read `<1s`, `12s`, then `1m 05s`.
export function took(ms: number): string {
  const secs = Math.floor(Math.max(0, ms) / 1000);
  if (secs === 0) return "<1s";
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`;
}

/** The place a seat row opens: Spec for planner on a two-room card, Build for foreman, its own DM otherwise. */
export function seatPlace(agent: string, cardId: string, twoRooms: boolean): string {
  if (agent.endsWith("/dm")) return agent;
  if (agent === `${cardId}/planner` && twoRooms) return `${cardId}/spec`;
  if (agent === `${cardId}/foreman`) return cardId;
  return `${agent}/dm`;
}

// Matches crates/types/src/config/seats.rs `Cli::efforts`: agy Opus lacks xhigh/max; other agy models lack max.
export function modelTakesEffort(cli: string, model: string, effort: string): boolean {
  if (cli !== "agy") return true;
  return model === "claude-opus-5-5" ? ["low", "medium", "high"].includes(effort) : effort !== "max";
}

export function clockTime(ms: number, tzOffsetMin: number): string {
  const d = new Date(ms + tzOffsetMin * 60_000);
  return `${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
}

export function thousands(n: number): string {
  return n >= 1000 ? `${Math.floor(n / 1000)}k` : String(n);
}

export function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

export function authorName(from: Author): string {
  if (from.is === "operator") return "Operator";
  if (from.is === "engine") return "engine";
  if (from.is === "orchestrator") return `orchestrator:${from.project}`;
  if (from.is === "pa") return "pa";
  return from.label;
}

// A DM session shows as `dm foreman`, others as their key's last part.
export function agentSeatName(agent: string): string {
  const dm = agent.endsWith("/dm");
  const base = dm ? agent.slice(0, -"/dm".length) : agent;
  const last = base.split("/").pop() ?? base;
  return dm ? `dm ${last}` : last;
}

// A hand's place below its card: `spec`, a role's `dm foreman`, a child's `researcher-2`, or a folded review place.
export function wherePlace(hand: HandView): string {
  if (hand.card === null || hand.message_place === hand.card) return "";
  if (hand.message_place.startsWith(`${hand.card}/`)) {
    const rest = hand.message_place.slice(hand.card.length + 1);
    if (!rest.endsWith("/dm")) return rest;
    const seat = rest.slice(0, -"/dm".length);
    return /-\d+$/.test(seat) ? seat : `dm ${seat}`;
  }
  const slash = hand.message_place.indexOf("/");
  if (slash < 0) return "Review";
  const rest = hand.message_place.slice(slash + 1);
  if (!rest.endsWith("/dm")) return `Review · ${rest}`;
  const seat = rest.slice(0, -"/dm".length);
  return `Review · ${/-\d+$/.test(seat) ? seat : `dm ${seat}`}`;
}

// A carried card opens the run carrying it.
export function cardTarget(row: BoardRow): string {
  return row.room ?? row.id;
}

// One label per working seat on a run: live.working plus each working child or DM seat in rooms.
export function workingSeats(row: BoardRow, rooms: RoomRow[]): string[] {
  const card = cardTarget(row);
  const extra = rooms.flatMap((r) => {
    const count = r.squares.filter((s) => s === "working").length;
    if (count === 0) return [];
    let label = "";
    if (r.kind.is === "child" && r.kind.card === card) label = r.kind.role ?? r.kind.agent.split("/").pop() ?? "";
    else if (r.kind.is === "dm" && r.kind.card === card) label = `dm ${r.kind.role}`;
    return label === "" ? [] : Array.from({ length: count }, () => label);
  });
  return [...(row.live?.working ?? []), ...extra].filter((s) => s !== "");
}

// The four status colours, only ever on a dot.
export const DOT: Record<Color, string> = {
  red: "bg-red-700",
  yellow: "bg-amber-700",
  green: "bg-green-700",
  grey: "bg-gray-500",
};
