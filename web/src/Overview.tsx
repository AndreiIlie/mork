import { type ReactNode, useState } from "react";
import { Markdown } from "./Markdown";
import { clockTime, plural, took } from "./format";
import { ChevronIcon } from "./icons";
import type {
  Color,
  FindingView,
  LiveRow,
  OverviewStage,
  OverviewStageAt,
  OverviewView,
  Plan,
  SuiteProgress,
} from "./types";
import { Dot } from "./ui/Dot";
import { PaneList, Row } from "./ui/List";
import { PinnedScroll } from "./ui/PinnedScroll";
import { ProgressBar } from "./ui/ProgressBar";

const MARK: Record<Exclude<OverviewStageAt, "current">, string> = { passed: "✓", failed: "✕" };
const DEEP = "pl-[calc(var(--spacing-gutter)+48px)] pr-gutter";

function minutes(ms: number): string {
  const m = Math.round(ms / 60_000);
  if (m < 1) return "<1m";
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

// Matches crates/ui/src/plan.rs `at`: deliverables before current are done, after are to come.
function taskAt(plan: Plan, deliverable: number, task: number): "done" | "current" | "to-come" {
  const currentTask = plan.deliverables[deliverable]?.current ?? 0;
  if (deliverable < plan.current || (deliverable === plan.current && task < currentTask)) return "done";
  if (deliverable === plan.current && task === currentTask) return "current";
  return "to-come";
}

function planCounts(plan: Plan | undefined): { done: number; total: number } {
  if (plan === undefined) return { done: 0, total: 0 };
  let done = 0;
  let total = 0;
  for (let i = 0; i < plan.deliverables.length; i += 1) {
    const tasks = plan.deliverables[i].tasks;
    total += tasks.length;
    for (let t = 0; t < tasks.length; t += 1) {
      if (taskAt(plan, i, t) === "done") done += 1;
    }
  }
  return { done, total };
}

function PlanRows({ plan }: { plan: Plan }) {
  const [folds, setFolds] = useState<Record<string, boolean>>({});
  return (
    <>
      {plan.deliverables.map((d, i) => {
        const open = folds[d.name] ?? i === plan.current;
        const done = d.tasks.filter((_, t) => taskAt(plan, i, t) === "done").length;
        return (
          <div key={d.name}>
            <Row pane indent={1} expanded={open} onClick={() => setFolds((p) => ({ ...p, [d.name]: !open }))}>
              <ChevronIcon open={open} className="text-gray-700" />
              <span
                className={`min-w-0 flex-1 truncate text-label-13 ${i === plan.current ? "font-medium text-gray-1000" : "text-gray-900"}`}
              >
                {d.name}
              </span>
              <span className="shrink-0 font-mono text-label-12 tabular-nums text-gray-700">
                {done}/{d.tasks.length}
              </span>
            </Row>
            {open && (
              <div className={`${DEEP} flex flex-col gap-1 pb-3`}>
                {d.tasks.map((task, t) => {
                  const at = taskAt(plan, i, t);
                  const tone = at === "current" ? "text-gray-1000" : at === "done" ? "text-gray-700" : "text-gray-900";
                  return (
                    <div key={`${t}:${task}`} className={`flex gap-2 text-label-13 ${tone}`}>
                      <span className="w-3 shrink-0 font-mono">{at === "done" ? "✓" : at === "current" ? "›" : "·"}</span>
                      <span className="min-w-0">{task}</span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

function FindingRows({ findings }: { findings: FindingView[] }) {
  const [open, setOpen] = useState<Record<number, boolean>>({});
  if (findings.length === 0) return null;
  return (
    <>
      {findings.map((f) => {
        const proof = (f.proof ?? "").trim();
        const hasProof = proof !== "";
        const shown = hasProof && (open[f.n] ?? false);
        const live = f.state === "open";
        const content = (
          <>
            {hasProof ? (
              <ChevronIcon open={shown} className="shrink-0 text-gray-700" />
            ) : (
              <span className="w-3 shrink-0" aria-hidden="true" />
            )}
            <span className={`w-7 shrink-0 font-mono text-label-12 ${live ? "text-gray-1000" : "text-gray-700"}`}>
              #{f.n}
            </span>
            <span className="w-[60px] shrink-0 font-mono text-label-12 text-gray-700">{f.state}</span>
            <span className="min-w-0 flex-1">
              <span className={`block text-label-13 ${live ? "text-gray-1000" : "text-gray-900"}`}>{f.summary}</span>
              <span className="mt-1 block truncate font-mono text-label-12 text-gray-700">
                {f.file}:{f.line} · {f.by}
              </span>
            </span>
          </>
        );
        return (
          <div key={f.n}>
            {hasProof ? (
              <Row pane indent={1} expanded={shown} onClick={() => setOpen((p) => ({ ...p, [f.n]: !shown }))}>
                {content}
              </Row>
            ) : (
              <Row pane indent={1}>
                {content}
              </Row>
            )}
            {shown && (
              <div className={`${DEEP} pb-3`}>
                <div className="border-l border-gray-400 pl-3">
                  <Markdown text={proof} className="text-gray-900" />
                </div>
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

/** A running suite's counts: passed of its total when known, else what passed, tests or Jest's files. */
function suiteCounts(p: SuiteProgress): string {
  const passed =
    p.total !== undefined
      ? `${p.passed} / ${p.total}`
      : p.files === true
        ? `${p.passed} ${plural(p.passed, "file", "files")}`
        : `${p.passed}`;
  return `${passed} passed · ${p.failed} failed`;
}

function LiveRows({ rows, nowMs }: { rows: LiveRow[]; nowMs: number }) {
  return (
    <>
      {rows.map((row, k) => {
        const time = row.start_ms !== undefined ? took((row.end_ms ?? nowMs) - row.start_ms) : "";
        const p = row.progress;
        const percent =
          p?.total !== undefined && p.total > 0 ? Math.min(100, Math.floor(((p.passed + p.failed) * 100) / p.total)) : null;
        const name = row.name === "Suite" ? "Test Suite" : row.name;
        return (
          <Row key={k} pane indent={row.under === true ? 2 : 1}>
            <div className="flex min-w-0 flex-1 flex-col gap-2">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-label-13">
                <span className="flex w-2 shrink-0 justify-center">{row.working === true && <Dot color="green" pulse />}</span>
                <span className={`w-28 shrink-0 truncate text-gray-1000 ${row.seat === true ? "font-mono" : "font-medium"}`}>
                  {name}
                </span>
                {row.topic !== undefined && <span className="w-40 shrink-0 truncate text-gray-900">{row.topic}</span>}
                {percent !== null && <span className="shrink-0 font-mono tabular-nums text-gray-1000">{percent}%</span>}
                {p !== undefined ? (
                  <span className="shrink-0 whitespace-nowrap tabular-nums text-gray-900">{suiteCounts(p)}</span>
                ) : (
                  <span className="min-w-0 flex-1 truncate tabular-nums text-gray-900">{row.words}</span>
                )}
                <span className="ml-auto shrink-0 font-mono text-label-12 tabular-nums text-gray-700">{time}</span>
              </div>
              {p !== undefined && percent !== null && <ProgressBar percent={percent} />}
              {p?.latest !== undefined && (
                <span className="truncate font-mono text-label-12 text-gray-700">{p.latest}</span>
              )}
            </div>
          </Row>
        );
      })}
    </>
  );
}

// What an unfolded stage shows: its live rows, its plan, its round's findings; Spec, and a stage with none, never fold.
function stageParts(
  stage: OverviewStage,
  {
    current,
    lastBuild,
    plan,
    hideFindings,
    live,
    nowMs,
  }: {
    current: boolean;
    lastBuild: boolean;
    plan: Plan | undefined;
    hideFindings: boolean;
    live: LiveRow[];
    nowMs: number;
  },
): ReactNode[] {
  const parts: ReactNode[] = [];
  if (stage.kind === "spec") return parts;
  if (current && live.length > 0) parts.push(<LiveRows key="live" rows={live} nowMs={nowMs} />);
  if (lastBuild && plan !== undefined && plan.deliverables.length > 0) parts.push(<PlanRows key="plan" plan={plan} />);
  const round = stage.round;
  if (!hideFindings && round !== undefined && round.findings.length > 0) {
    parts.push(<FindingRows key="findings" findings={round.findings} />);
  }
  return parts;
}

/** Per-card Overview pane: full-bleed stage rows with inline live status, plan, and findings with their proofs. */
export function Overview({
  overview,
  plan,
  triageRound,
  tzOffsetMin,
  status,
  nowMs,
}: {
  overview: OverviewView | undefined;
  plan: Plan | undefined;
  triageRound: number | undefined;
  tzOffsetMin: number;
  status: Color;
  nowMs: number;
}) {
  const stages = overview?.stages ?? [];
  const live = overview?.live ?? [];
  // Key round stages by round number so a replacement round never inherits a superseded round's fold state.
  const [folds, setFolds] = useState<Record<string, boolean>>({});
  const stageKey = (st: OverviewStage, index: number) =>
    st.round !== undefined ? `${st.kind}:r${st.round.n}` : `${st.kind}:${index}`;
  const lastBuild = stages.map((s) => s.kind).lastIndexOf("build");
  const { done, total } = planCounts(plan);

  return (
    <PinnedScroll>
      {stages.length === 0 ? (
        <p className="px-gutter py-4 text-label-13 text-gray-700">No stages yet.</p>
      ) : (
        <PaneList>
          {stages.map((st, i) => {
            const current = st.at === "current";
            const key = stageKey(st, i);
            const round = st.round;
            const parts = stageParts(st, {
              current,
              lastBuild: i === lastBuild,
              plan,
              hideFindings: round !== undefined && round.n === triageRound,
              live,
              nowMs,
            });
            const foldable = parts.length > 0;
            const open = foldable && (folds[key] ?? current);
            return (
              <div key={key}>
                <Row
                  pane
                  expanded={foldable ? open : undefined}
                  active={current && !open}
                  onClick={foldable ? () => setFolds((prev) => ({ ...prev, [key]: !open })) : undefined}
                >
                  <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1">
                    <span className="flex w-3 shrink-0 items-center justify-center font-mono text-label-12 text-gray-1000">
                      {st.at === "current" ? <Dot color={status} pulse={status === "green"} /> : MARK[st.at]}
                    </span>
                    <span className="shrink-0 font-medium text-gray-1000">{st.label}</span>
                    {st.summary !== undefined && st.summary !== "" && (
                      <span className="min-w-0 truncate text-label-13 text-gray-900">{st.summary}</span>
                    )}
                    {i === lastBuild && total > 0 && (
                      <span className="w-16 shrink-0">
                        <ProgressBar percent={(done * 100) / total} tone="strong" />
                      </span>
                    )}
                    {round !== undefined && (
                      <span className="shrink-0 font-mono text-label-12 text-gray-700">{round.commit}</span>
                    )}
                    <span className="ml-auto shrink-0 font-mono text-label-12 tabular-nums text-gray-700">
                      {clockTime(st.start_ms, tzOffsetMin)} · {minutes((st.end_ms ?? nowMs) - st.start_ms)}
                    </span>
                  </div>
                  {foldable ? (
                    <ChevronIcon open={open} className="shrink-0 text-gray-700" />
                  ) : (
                    <span className="w-3.5 shrink-0" aria-hidden="true" />
                  )}
                </Row>
                {open && <div className="divide-y divide-gray-400 border-t border-gray-400 bg-background-100">{parts}</div>}
              </div>
            );
          })}
        </PaneList>
      )}
    </PinnedScroll>
  );
}
