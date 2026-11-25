import type { StageMark } from "./types";

function stepLabel(s: StageMark): string {
  if (s.count === undefined) return s.label;
  return s.kind === "build" ? `${s.label} ${s.count}` : `${s.label} · ${s.count}`;
}

/** The card's steps on one plain line: passed ones ticked, the current one bright with its sub-status. */
export function Pipeline({ steps }: { steps: StageMark[] }) {
  return (
    <ol className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-label-13">
      {steps.map((s, i) => {
        const tone = s.at === "current" ? "font-medium text-gray-1000" : "text-gray-700";
        return (
          <li key={s.label} className="flex items-center gap-3">
            {i > 0 && <span aria-hidden="true" className="h-px w-6 bg-gray-400" />}
            <span className={`flex h-6 items-center gap-2 ${tone}`}>
              {s.at === "passed" && <span className="font-mono text-label-12">✓</span>}
              <span>{stepLabel(s)}</span>
            </span>
            {s.sub !== undefined && <span className="font-mono text-label-12 text-gray-900">{s.sub}</span>}
          </li>
        );
      })}
    </ol>
  );
}
