import type { ReactNode } from "react";

const HEIGHT = {
  72: "h-[72px]",
  156: "h-[156px]",
} as const;

export type ChartHeight = keyof typeof HEIGHT;
export type ChartXAlign = "between" | "columns";
export type ChartXLabel = string | { full: string; short: string };

export interface ChartLayoutProps {
  yLabels: readonly string[];
  xLabels: readonly ChartXLabel[];
  height?: ChartHeight;
  headroom?: boolean;
  xAlign?: ChartXAlign;
  children: ReactNode;
}

/** Stretched plot with HTML axis labels around it so SVG text is never stretched. */
export function ChartLayout({
  yLabels,
  xLabels,
  height = 156,
  headroom = false,
  xAlign = "between",
  children,
}: ChartLayoutProps) {
  return (
    <div className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-1 font-mono text-label-12 tabular-nums text-gray-700">
      <div className={`flex flex-col justify-between text-right ${headroom ? "pt-4" : ""}`}>
        {yLabels.map((l, i) => (
          <span key={`${i}:${l}`}>{l}</span>
        ))}
      </div>
      <div className={`min-w-0 ${HEIGHT[height]} ${headroom ? "pt-4" : ""} border-b border-l border-gray-400`}>{children}</div>
      <span />
      <div className={xAlign === "columns" ? "flex min-w-0" : "flex min-w-0 justify-between"}>
        {xLabels.map((l, i) => {
          const full = typeof l === "string" ? l : l.full;
          const short = typeof l === "string" ? null : l.short;
          return (
            <span
              key={`${i}:${full}`}
              title={short !== null ? full : undefined}
              className={xAlign === "columns" ? "min-w-0 flex-1 truncate text-center" : undefined}
            >
              {short !== null ? (
                <>
                  <span className="hidden lg:inline">{full}</span>
                  <span className="lg:hidden">{short}</span>
                </>
              ) : (
                full
              )}
            </span>
          );
        })}
      </div>
    </div>
  );
}
