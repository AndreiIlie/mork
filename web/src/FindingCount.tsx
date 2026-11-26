import type { ReviewerCounts } from "./types";

export function FindingCount({ count, verdict }: { count: ReviewerCounts; verdict: string }) {
  const { kept, reported } = count;
  const report = `${reported} reported.`;
  const title = kept !== undefined ? `${kept} kept by the review foreman. ${report}`
    : verdict === "superseded" ? `${report} Round superseded.`
    : `${report} Awaiting the review foreman’s verdict.`;
  return (
    <span role="img" aria-label={title} title={title} className="inline-flex shrink-0 items-center font-mono text-label-12 tabular-nums">
      {kept !== undefined && <>
        <span className={kept > 0 ? "text-red-700" : "text-gray-700"}>{kept}</span>
        <span className="text-gray-1000">/</span>
      </>}
      <span className={reported > 0 ? "text-amber-700" : "text-gray-700"}>{reported}</span>
    </span>
  );
}
