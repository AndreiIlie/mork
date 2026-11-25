const TONE = {
  neutral: "bg-gray-900",
  ok: "bg-green-700",
  hot: "bg-amber-700",
  strong: "bg-gray-1000",
} as const;

const HEIGHT = {
  2: "h-0.5",
  4: "h-1",
  6: "h-1.5",
} as const;

export type ProgressTone = keyof typeof TONE;
export type ProgressHeight = keyof typeof HEIGHT;

export interface ProgressBarProps {
  percent: number;
  tone?: ProgressTone;
  tick?: number | null;
  height?: ProgressHeight;
}

/** Sharp progress track with a toned fill and an optional vertical elapsed tick. */
export function ProgressBar({ percent, tone = "neutral", tick = null, height = 4 }: ProgressBarProps) {
  const fill = Math.max(0, Math.min(100, percent));
  const mark = tick !== null ? Math.max(0, Math.min(100, tick)) : null;
  return (
    <div className={`relative w-full bg-gray-200 ${HEIGHT[height]}`}>
      <div className={`h-full ${TONE[tone]}`} style={{ width: `${fill}%` }} />
      {mark !== null && <div className="absolute -bottom-0.5 -top-0.5 w-px bg-gray-1000" style={{ left: `${mark}%` }} />}
    </div>
  );
}
