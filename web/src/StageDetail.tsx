import { useState } from "react";
import { Markdown } from "./Markdown";
import { FindingCount } from "./FindingCount";
import { clockTime, plural, took } from "./format";
import { ChevronIcon } from "./icons";
import type { Color, FindingView, LiveRow, Plan, SuiteProgress } from "./types";
import { Dot } from "./ui/Dot";
import { PaneList, Row } from "./ui/List";
import { ProgressBar } from "./ui/ProgressBar";
import type { StageNode } from "./CardTree";

const HEAD = "px-gutter pb-2 pt-5 text-heading-14 text-gray-1000";
const DEEP = "pl-[calc(var(--spacing-gutter)+48px)] pr-gutter";

function minutes(ms: number): string {
  const m = Math.round(ms / 60_000);
  if (m < 1) return "<1m";
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

function taskAt(plan: Plan, d: number, t: number): "done" | "current" | "to-come" {
  const currentTask = plan.deliverables[d]?.current ?? 0;
  if (d < plan.current || (d === plan.current && t < currentTask)) return "done";
  if (d === plan.current && t === currentTask) return "current";
  return "to-come";
}

function PlanRows({ plan }: { plan: Plan }) {
  const [folds, setFolds] = useState<Record<string, boolean>>({});
  return (
    <PaneList>
      {plan.deliverables.map((d, i) => {
        const open = folds[d.name] ?? i === plan.current;
        const done = d.tasks.filter((_, t) => taskAt(plan, i, t) === "done").length;
        return (
          <div key={d.name}>
            <Row pane expanded={open} onClick={() => setFolds((p) => ({ ...p, [d.name]: !open }))}>
              <ChevronIcon open={open} className="text-gray-700" />
              <span className={`min-w-0 flex-1 truncate ${i === plan.current ? "font-medium text-gray-1000" : "text-gray-900"}`}>{d.name}</span>
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
                    <div key={task} className={`flex gap-2 text-label-13 ${tone}`}>
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
    </PaneList>
  );
}

function FindingRows({ findings }: { findings: FindingView[] }) {
  const [open, setOpen] = useState<Record<number, boolean>>({});
  return (
    <PaneList>
      {findings.map((f) => {
        const proof = (f.proof ?? "").trim();
        const shown = proof !== "" && (open[f.n] ?? false);
        const live = f.state === "open";
        const content = (
          <>
            {proof !== "" ? <ChevronIcon open={shown} className="shrink-0 text-gray-700" /> : <span className="w-3 shrink-0" aria-hidden="true" />}
            <span className={`w-7 shrink-0 font-mono text-label-12 ${live ? "text-gray-1000" : "text-gray-700"}`}>#{f.n}</span>
            <span className="w-[60px] shrink-0 font-mono text-label-12 text-gray-700">{f.state}</span>
            <span className="min-w-0 flex-1">
              <span className={`block text-label-14 ${live ? "text-gray-1000" : "text-gray-900"}`}>{f.summary}</span>
              <span className="mt-1 block truncate font-mono text-label-12 text-gray-700">
                {f.file}:{f.line} · {f.by}
              </span>
            </span>
          </>
        );
        return (
          <div key={f.n}>
            {proof !== "" ? (
              <Row pane expanded={shown} onClick={() => setOpen((p) => ({ ...p, [f.n]: !shown }))}>
                {content}
              </Row>
            ) : (
              <Row pane>{content}</Row>
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
    </PaneList>
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

function StepRows({ rows, nowMs }: { rows: LiveRow[]; nowMs: number }) {
  return (
    <PaneList>
      {rows.map((row) => {
        const p = row.progress;
        const percent = p?.total !== undefined && p.total > 0 ? Math.floor(((p.passed + p.failed) * 100) / p.total) : null;
        const time = row.start_ms !== undefined ? took((row.end_ms ?? nowMs) - row.start_ms) : "";
        return (
          <Row key={row.name} pane>
            <div className="flex min-w-0 flex-1 flex-col gap-2">
              <div className="flex items-center gap-3 text-label-14">
                <span className="flex w-2 shrink-0 justify-center">{row.working === true && <Dot color="green" pulse />}</span>
                <span className="w-28 shrink-0 font-medium text-gray-1000">{row.name === "Suite" ? "Test Suite" : row.name}</span>
                {percent !== null && <span className="shrink-0 font-mono text-label-13 tabular-nums text-gray-1000">{percent}%</span>}
                <span className="min-w-0 flex-1 truncate text-label-13 tabular-nums text-gray-900">
                  {p !== undefined ? suiteCounts(p) : row.words}
                </span>
                <span className="shrink-0 font-mono text-label-12 tabular-nums text-gray-700">{time}</span>
              </div>
              {percent !== null && <ProgressBar percent={percent} />}
              {p?.latest !== undefined && <span className="truncate font-mono text-label-12 text-gray-700">{p.latest}</span>}
            </div>
          </Row>
        );
      })}
    </PaneList>
  );
}

export function StageDetail({
  node,
  status,
  decisions,
  triageRound,
  plan,
  steps,
  tzOffsetMin,
  nowMs,
}: {
  node: StageNode;
  status: Color;
  decisions: string[];
  triageRound?: number;
  plan: Plan | undefined;
  steps: LiveRow[];
  tzOffsetMin: number;
  nowMs: number;
}) {
  const st = node.stage;
  const findings = st?.round?.n === triageRound ? [] : st?.round?.findings ?? [];
  const round = st?.round;
  const reviewers = Object.entries(round?.reviewers ?? {});
  const empty = node.at === "to-come" || (node.kind !== "spec" && plan === undefined && findings.length === 0 && steps.length === 0 && reviewers.length === 0);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-gray-400 px-gutter py-3">
        <span className="flex w-3 shrink-0 items-center justify-center font-mono text-label-12">
          {node.at === "passed" && <span className="text-green-700">✓</span>}
          {node.at === "failed" && <span className="text-red-700">✕</span>}
          {node.at === "current" && <Dot color={status} pulse={status === "green"} />}
        </span>
        <h2 className="text-label-14 font-medium text-gray-1000">{node.label}</h2>
        {st?.summary !== undefined && <span className="text-label-13 text-gray-900">{st.summary}</span>}
        {st?.round !== undefined && <span className="font-mono text-label-12 text-gray-700">{st.round.commit}</span>}
        {st !== undefined && (
          <span className="ml-auto font-mono text-label-12 tabular-nums text-gray-900">
            {clockTime(st.start_ms, tzOffsetMin)} · {minutes((st.end_ms ?? nowMs) - st.start_ms)}
          </span>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {node.kind === "spec" && (
          <>
            <h3 className={HEAD}>Decisions</h3>
            <ol className="border-y border-gray-400 divide-y divide-gray-400">
              {decisions.map((d, i) => (
                <li key={i} className="flex gap-3 px-gutter py-3 text-copy-14 text-gray-1000">
                  <span className="w-5 shrink-0 font-mono text-label-12 tabular-nums leading-5 text-gray-700">{i + 1}</span>
                  <span className="min-w-0 max-w-[86ch]">{d}</span>
                </li>
              ))}
            </ol>
          </>
        )}
        {steps.length > 0 && (
          <>
            <h3 className={HEAD}>Checks and suite</h3>
            <div className="border-t border-gray-400">
              <StepRows rows={steps} nowMs={nowMs} />
            </div>
          </>
        )}
        {plan !== undefined && node.kind === "build" && (
          <>
            <h3 className={HEAD}>Plan</h3>
            <div className="border-t border-gray-400">
              <PlanRows plan={plan} />
            </div>
          </>
        )}
        {round && reviewers.length > 0 && (
          <>
            <h3 className={HEAD}>Reviewers</h3>
            <div className="border-t border-gray-400">
              <PaneList>
                {reviewers.map(([name, count]) => (
                  <Row key={name} pane>
                    <span className="font-mono text-label-13 text-gray-900">{name}</span>
                    <FindingCount count={count} verdict={round.verdict} />
                  </Row>
                ))}
              </PaneList>
            </div>
          </>
        )}
        {findings.length > 0 && (
          <>
            <h3 className={HEAD}>Findings</h3>
            <div className="border-t border-gray-400">
              <FindingRows findings={findings} />
            </div>
          </>
        )}
        {empty && <p className="px-gutter py-5 text-copy-14 text-gray-700">{node.at === "to-come" ? "Not started yet." : "Nothing to show for this stage."}</p>}
      </div>
    </div>
  );
}
