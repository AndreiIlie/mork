import type { ReactNode } from "react";
import { HandIcon } from "./icons";
import { ContextRing } from "./ContextRing";
import { FindingCount } from "./FindingCount";
import { SeatDot } from "./SeatChat";
import { Dot } from "./ui/Dot";
import type { AgentRow, CardHeader, Color, LiveRow, OverviewStage, PlaceView, StageMark } from "./types";

export interface StageNode {
  key: string;
  kind: string;
  label: string;
  at: "passed" | "failed" | "current" | "to-come";
  note: string;
  stage?: OverviewStage;
}

interface SeatNode {
  row: AgentRow;
  depth: number;
}

// Every stage of the card in run order: those it visited, each with the engine's own summary, then those still to come.
export function stageNodes(view: PlaceView, card: CardHeader): StageNode[] {
  const visited = view.overview?.stages ?? [];
  const nodes: StageNode[] = visited.map((st, i) => ({
    key: `${st.kind}:${i}`,
    kind: st.kind,
    label: st.label,
    at: st.at,
    note: st.summary ?? "",
    stage: st,
  }));
  const lastRound = Math.max(0, ...visited.map((st) => st.round?.n ?? 0));
  for (const p of card.pipeline.filter((s: StageMark) => s.at === "to-come")) {
    const label = p.kind === "verify" && lastRound > 0 ? `Verify · Round ${lastRound + 1}` : p.label;
    nodes.push({ key: `${p.kind}:next`, kind: p.kind, label, at: "to-come", note: "" });
  }
  return nodes;
}

export function lastKey(nodes: StageNode[], kind: string, visitedOnly = false): string | undefined {
  const pool = visitedOnly ? nodes.filter((n) => n.at !== "to-come") : nodes;
  return [...pool].reverse().find((n) => n.kind === kind)?.key;
}

function StageMarkCell({ at, status }: { at: StageNode["at"]; status: Color }) {
  let mark: ReactNode = null;
  if (at === "passed") mark = <span className="text-green-700">✓</span>;
  else if (at === "failed") mark = <span className="text-red-700">✕</span>;
  else if (at === "current") mark = <Dot color={status} pulse={status === "green"} />;
  return <span className="flex w-3 shrink-0 items-center justify-center font-mono text-label-12">{mark}</span>;
}

const STAGE_TONE: Record<StageNode["at"], string> = {
  passed: "text-gray-950",
  failed: "text-gray-950",
  current: "font-medium text-gray-1000",
  "to-come": "text-gray-600",
};

interface TreeProps {
  nodes: StageNode[];
  seats: AgentRow[];
  card: CardHeader;
  review?: string;
  steps: LiveRow[];
  status: Color;
  selected: string;
  pick: (selection: string) => void;
  seatName: (key: string) => string;
  markOf: (key: string) => string | undefined;
}

function stageSeats(nodes: StageNode[], seats: AgentRow[], card: CardHeader, review?: string) {
  const groups: Record<string, SeatNode[]> = {};
  const spec = lastKey(nodes, "spec");
  const build = lastKey(nodes, "build", !card.spec) ?? spec;
  const own = card.mode === "report" ? lastKey(nodes, "report") : card.mode === "review" ? lastKey(nodes, "review") : build;
  const verify = lastKey(nodes, "verify", true) ?? lastKey(nodes, "verify");
  const byId = new Map(seats.map((s) => [s.agent, s]));
  const children = new Map<string, AgentRow[]>();
  const roots: AgentRow[] = [];
  for (const row of seats) {
    const dmOf = row.agent.endsWith("/dm") ? row.agent.slice(0, -3) : undefined;
    const parent = dmOf ?? row.parent;
    if (parent && byId.has(parent) && row.agent !== card.id + "/planner") {
      const list = children.get(parent) ?? [];
      list.push(row);
      children.set(parent, list);
    } else roots.push(row);
  }
  const append = (stage: string, row: AgentRow, depth: number) => {
    (groups[stage] ??= []).push({ row, depth });
    for (const child of children.get(row.agent) ?? []) append(stage, child, depth + 1);
  };
  for (const row of roots) {
    const folded = review && row.agent.startsWith(review + "/");
    const stage = folded ? verify : row.role === "planner" ? spec : own;
    if (stage) append(stage, row, 1);
  }
  return groups;
}

export function CardTree({ nodes, card, seats, review, steps, status, selected, pick, seatName, markOf }: TreeProps) {
  const under = stageSeats(nodes, seats, card, review);
  return (
    <nav aria-label="Card Tree" className="w-[360px] shrink-0 overflow-y-auto border-r border-gray-400">
      {nodes.map((n) => {
        const on = selected === `stage ${n.key}`;
        const round = n.stage?.round;
        return (
          <div key={n.key} className="border-b border-gray-400">
            <button
              type="button"
              onClick={() => pick(`stage ${n.key}`)}
              className={`flex h-9 w-full cursor-pointer items-center gap-2 pl-gutter pr-edge text-left text-label-13 transition-colors ${on ? "bg-gray-200" : "hover:bg-gray-100"}`}
            >
              <StageMarkCell at={n.at} status={status} />
              <span className={`shrink-0 ${on && n.at !== "to-come" ? "font-medium text-gray-1000" : STAGE_TONE[n.at]}`}>{n.label}</span>
              <span className="ml-auto min-w-0 truncate font-mono text-label-12 tabular-nums text-gray-700">{n.note}</span>
            </button>
            {n.at === "current" && n.kind === "verify" &&
              steps.map((s) => (
                <button
                  key={s.name}
                  type="button"
                  onClick={() => pick(`stage ${n.key}`)}
                  className="flex h-7 w-full cursor-pointer items-center gap-2 pl-[44px] pr-edge text-left text-label-12 transition-colors hover:bg-gray-100"
                >
                  <span className="flex w-2 shrink-0 justify-center">{s.working === true && <Dot color="green" pulse />}</span>
                  <span className="text-gray-900">{s.name === "Suite" ? "Test Suite" : s.name}</span>
                  <span className="ml-auto shrink-0 font-mono tabular-nums text-gray-700">
                    {s.progress?.total !== undefined && s.progress.total > 0
                      ? `${Math.floor(((s.progress.passed + s.progress.failed) * 100) / s.progress.total)}%`
                      : s.words.toLowerCase()}
                  </span>
                </button>
              ))}
            {(under[n.key] ?? []).map(({ row, depth }) => {
              const seatOn = selected === `seat ${row.agent}`;
              const lit = row.light !== "idle";
              const mark = markOf(row.agent);
              const name = seatName(row.agent);
              const count = row.role === "reviewer" ? round?.reviewers?.[name] : undefined;
              return (
                <button
                  key={row.agent}
                  type="button"
                  onClick={() => pick(`seat ${row.agent}`)}
                  className={`flex min-h-7 w-full cursor-pointer flex-wrap items-center gap-x-2 py-1 pr-edge text-left text-label-12 transition-colors ${
                    depth === 1 ? "pl-[44px]" : depth === 2 ? "pl-[64px]" : "pl-[84px]"
                  } ${seatOn ? "bg-gray-200" : "hover:bg-gray-100"}`}
                >
                  <SeatDot light={row.light} />
                  <span className={`shrink-0 font-mono ${seatOn || lit ? "font-medium text-gray-1000" : "text-gray-900"}`}>{name}</span>
                  {count && round && <FindingCount count={count} verdict={round.verdict} />}
                  {mark !== undefined && (
                    <span aria-label="Raised Hand" className={mark}>
                      <HandIcon className="h-3 w-3" />
                    </span>
                  )}
                  <span className="ml-auto flex min-w-0 max-w-full items-center gap-2 text-right text-gray-700">
                    <span className="min-w-0 [overflow-wrap:anywhere]">
                      {row.label}{row.effort && ` · ${row.effort}`}
                    </span>
                    <ContextRing context={row.context} />
                  </span>
                </button>
              );
            })}
          </div>
        );
      })}
    </nav>
  );
}
