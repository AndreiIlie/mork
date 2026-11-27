import { Fragment, useState } from "react";
import { agentSeatName, clockTime, plural, took } from "./format";
import { ChevronIcon } from "./icons";
import type { ChatTurn, Detail, StepView } from "./types";
import { Dot } from "./ui/Dot";

// An unfolded turn shows its latest steps only, never a scrolling list.
const STEPS_SHOWN = 8;
const LIVE_STEPS_SHOWN = 3;
const NOTE = "py-0.5 text-gray-700";

function DetailRow({ d }: { d: Detail }) {
  const [open, setOpen] = useState(false);
  const first = d.text.split("\n", 1)[0] ?? "";
  const kind = (d.sub ? "subagent's " : "") + d.kind;
  return (
    <li data-unfold-row className="py-0.5 pl-[52px]">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="flex w-full cursor-pointer items-baseline gap-2 text-left text-gray-900 hover:text-gray-1000"
      >
        <ChevronIcon open={open} className="translate-y-[2px] text-gray-700" />
        <span className="shrink-0 font-mono text-gray-700">{kind}</span>
        {!open && <span className="min-w-0 flex-1 truncate">{first}</span>}
      </button>
      {open && (
        <div className="ml-[22px] mt-1 max-w-[74ch] whitespace-pre-wrap border-l border-gray-400 pl-3 text-label-13 text-gray-900">
          {d.text}
          {d.cut && <p className="mt-1 text-gray-700">the text goes on past what is kept</p>}
        </div>
      )}
    </li>
  );
}

function StepRow({ st, before, runs, nowMs }: { st: StepView; before: number; runs: boolean; nowMs: number }) {
  const [open, setOpen] = useState(false);
  const raw = st.command?.text ? st.command.text + (st.command.cut ? "…" : "") : undefined;
  return (
    <li data-unfold-row>
      <button
        type="button"
        title={raw}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={`flex w-full cursor-pointer items-baseline gap-3 py-0.5 text-left hover:text-gray-1000 ${runs ? "text-gray-1000" : "text-gray-900"}`}
      >
        <span className={`min-w-0 flex-1 ${open ? "whitespace-pre-wrap break-words font-mono" : "truncate"}`}>
          {open ? raw ?? st.words : st.words}
        </span>
        <span className="shrink-0 font-mono tabular-nums text-gray-700">
          {st.start_ms === undefined ? "" : took(before + (st.end_ms ?? nowMs) - st.start_ms)}
        </span>
      </button>
    </li>
  );
}

function thinkingMs(turn: ChatTurn, nowMs: number): number | null {
  const since = turn.turn.thinking_since;
  if (!turn.running || since === undefined || nowMs - since < 1000) return null;
  return nowMs - since;
}

interface Folded {
  st: StepView;
  times: number;
  /** The durations of the steps folded before its last, whose own start and end it keeps. */
  before: number;
}

// A repeated label can hide different commands. Fold only matching, uncut commands with no new details.
function foldRepeats(steps: StepView[]): Folded[] {
  const out: Folded[] = [];
  for (const st of steps) {
    const last = out[out.length - 1];
    if (
      last !== undefined &&
      last.st.words === st.words &&
      last.st.command?.text === st.command?.text &&
      !last.st.command?.cut &&
      !st.command?.cut &&
      (st.details ?? []).length === 0
    ) {
      const prev = last.st;
      const done = prev.start_ms !== undefined && prev.end_ms !== undefined ? prev.end_ms - prev.start_ms : 0;
      const thought = (prev.thought_ms ?? 0) + (st.thought_ms ?? 0);
      last.times += 1;
      last.before += done;
      last.st = { ...prev, start_ms: st.start_ms, end_ms: st.end_ms, thought_ms: thought > 0 ? thought : undefined };
    } else {
      out.push({ st, times: 1, before: 0 });
    }
  }
  return out;
}

function TurnSteps({ turn, nowMs, compact = false }: { turn: ChatTurn; nowMs: number; compact?: boolean }) {
  const t = turn.turn;
  const shown = foldRepeats(t.steps).slice(-(compact ? LIVE_STEPS_SHOWN : STEPS_SHOWN));
  const hidden = Math.max(0, turn.step_count - shown.reduce((n, r) => n + r.times, 0));
  const thinking = thinkingMs(turn, nowMs);
  return (
    <ol className="border-l border-gray-400 pl-3 text-label-12">
      {!compact && hidden > 0 && (
        <li className="py-0.5 text-gray-700">
          {hidden} earlier {plural(hidden, "step", "steps")}
        </li>
      )}
      {!compact && shown.length === 0 && hidden === 0 && <li className="py-0.5 text-gray-700">No steps yet.</li>}
      {shown.map(({ st, times, before }, i) => (
        <Fragment key={st.id}>
          {!compact && st.thought_ms !== undefined && <li className={NOTE}>Thought for {took(st.thought_ms)}</li>}
          {!compact && (st.details ?? []).map((d) => (
            <DetailRow key={d.id} d={d} />
          ))}
          <StepRow
            st={times > 1 ? { ...st, words: `${st.words} ×${times}` } : st}
            before={before}
            runs={turn.running && i === shown.length - 1 && st.end_ms === undefined}
            nowMs={nowMs}
          />
        </Fragment>
      ))}
      {!compact && t.thought_ms !== undefined && <li className={NOTE}>Thought for {took(t.thought_ms)}</li>}
      {!compact && (t.after ?? []).map((d) => (
        <DetailRow key={d.id} d={d} />
      ))}
      {!compact && thinking !== null && <li className={`${NOTE} text-gray-1000`}>Thinking… {took(thinking)}</li>}
    </ol>
  );
}

// Includes model thinking before the first step and after the last; omits start only when earlier steps were dropped.
function turnBounds(turn: ChatTurn, nowMs: number): { start?: number; end?: number } {
  const steps = turn.turn.steps;
  const partial = turn.step_count > steps.length;
  const first = steps[0];
  const start = partial
    ? undefined
    : first?.start_ms !== undefined
      ? first.start_ms - (first.thought_ms ?? 0)
      : turn.running
        ? turn.turn.thinking_since
        : undefined;
  if (turn.running) return { start, end: nowMs };
  let lastEnd: number | undefined;
  for (const s of steps) {
    if (s.end_ms !== undefined && (lastEnd === undefined || s.end_ms > lastEnd)) lastEnd = s.end_ms;
  }
  const end = lastEnd !== undefined ? lastEnd + (turn.turn.thought_ms ?? 0) : undefined;
  return { start, end };
}

function turnNote(turn: ChatTurn, nowMs: number): string {
  const { start, end } = turnBounds(turn, nowMs);
  const count = `${turn.step_count} ${plural(turn.step_count, "step", "steps")}`;
  return start !== undefined && end !== undefined ? `${count} · ${took(end - start)}` : count;
}

/** One ended turn folded like a marker line where it ran in its stream; clicking unfolds its latest 8 steps. */
export function EndedTurnRow({ reviewForeman, turn, tzOffsetMin, nowMs }: { reviewForeman?: string; turn: ChatTurn; tzOffsetMin: number; nowMs: number }) {
  const [open, setOpen] = useState(false);
  const { end } = turnBounds(turn, nowMs);
  return (
    <div data-unfold-row className="py-2">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full cursor-pointer items-center gap-3 text-left text-label-12 text-gray-700 hover:text-gray-900"
      >
        <span className="shrink-0 font-mono">{turn.agent === reviewForeman ? "review foreman" : agentSeatName(turn.agent)} worked</span>
        <span className="min-w-0 truncate">{turnNote(turn, nowMs)}</span>
        <span className="h-px min-w-6 flex-1 bg-gray-400" />
        {end !== undefined && <span className="shrink-0 font-mono tabular-nums">{clockTime(end, tzOffsetMin)}</span>}
      </button>
      {open && (
        <div className="mt-2">
          <TurnSteps turn={turn} nowMs={nowMs} />
        </div>
      )}
    </div>
  );
}

/** The live turn shows three recent actions until expanded. */
export function ActivityTracker({ reviewForeman, turn, nowMs }: { reviewForeman?: string; turn: ChatTurn; nowMs: number }) {
  const [open, setOpen] = useState(false);
  const steps = turn.turn.steps;
  const thinking = thinkingMs(turn, nowMs);
  const status = thinking !== null ? `Thinking… ${took(thinking)}` : steps.length === 0 ? "working" : "";
  return (
    <div data-unfold-row className="py-1">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex h-9 w-full cursor-pointer items-center gap-3 text-left text-label-12 text-gray-900 hover:text-gray-1000"
      >
        <Dot color="green" pulse />
        <span className="shrink-0 font-mono text-gray-1000">{turn.agent === reviewForeman ? "review foreman" : agentSeatName(turn.agent)}</span>
        <span className="min-w-0 flex-1 truncate">{open ? "" : status}</span>
        <span className="shrink-0 font-mono tabular-nums text-gray-700">{turnNote(turn, nowMs)}</span>
        <ChevronIcon open={open} className="text-gray-700" />
      </button>
      {(open || steps.length > 0) && (
        <div className="pb-3 pl-5">
          <TurnSteps turn={turn} nowMs={nowMs} compact={!open} />
        </div>
      )}
    </div>
  );
}
