import { useEffect, useRef, useState } from "react";
import { readHistory, watchMain } from "./bridge";
import { clockTime } from "./format";
import { ChevronIcon } from "./icons";
import { useRetainedStream } from "./streamHistory";
import type { MessageView, PlaceView } from "./types";

const DATE = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });

function updates(messages: MessageView[]): MessageView[] {
  return messages.filter((m) =>
    m.from.is === "engine" && m.kind === "engine" && m.to === "operator" &&
    /^(Reloaded to |The reload failed:|The reload to |The build of )/.test(m.text)
  ).reverse();
}

interface HistoryProps {
  view: PlaceView;
  live: boolean;
  tzOffsetMin: number;
}

function History({ view, live, tzOffsetMin }: HistoryProps) {
  const { messages, earlier, firstSeq, prepend } = useRetainedStream(view);
  const entries = updates(messages);
  const [error, setError] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const hasEarlier = earlier > 0;
  useEffect(() => {
    const target = bottom.current;
    if (!live || !target || !hasEarlier || error !== null) return;
    let active = true;
    const observer = new IntersectionObserver(([entry]) => {
      if (!active || !entry.isIntersecting) return;
      observer.disconnect();
      readHistory("main", firstSeq).then((page) => {
        if (!active) return;
        if (page.earlier > 0 && page.messages.length === 0) throw new Error("Could not read earlier main-room messages.");
        prepend(page);
      }).catch((e: unknown) => {
        if (active) setError(e instanceof Error ? e.message : String(e));
      });
    }, { root: target.parentElement });
    observer.observe(target);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [error, firstSeq, hasEarlier, live, prepend]);

  return (
    <div className="max-h-[min(28rem,65vh)] overflow-y-auto">
      <ol className="divide-y divide-gray-400">
        {entries.map((m) => (
          <li key={m.seq} className="px-edge py-3">
            <time dateTime={new Date(m.time_ms).toISOString()} className="mb-1 block font-mono text-label-12 tabular-nums text-gray-700">
              {DATE.format(m.time_ms + tzOffsetMin * 60_000)}, {clockTime(m.time_ms, tzOffsetMin)}
            </time>
            <div className="whitespace-pre-wrap wrap-anywhere text-copy-14">{m.text}{m.cut && "…"}</div>
          </li>
        ))}
      </ol>
      {!hasEarlier && entries.length === 0 && (
        <p className="px-edge py-3 text-label-13 text-gray-700">No reloads or build updates yet.</p>
      )}
      {hasEarlier && error === null && (
        <div ref={bottom} role="status" className="px-edge py-2 text-label-12 text-gray-700">
          {live ? "Loading earlier updates…" : "Reconnecting…"}
        </div>
      )}
      {error !== null && <p role="alert" className="px-edge py-2 text-copy-13 text-red-900">{error}</p>}
    </div>
  );
}

interface ChangelogProps {
  build: string;
  view: PlaceView | undefined;
  live: boolean;
  tzOffsetMin: number;
}

export function ReloadChangelog({ build, view, live, tzOffsetMin }: ChangelogProps) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    watchMain(true);
    return () => watchMain(false);
  }, [open]);

  return (
    <>
      <button
        type="button"
        popoverTarget="reload-history"
        title="Reload changelog"
        aria-label={`Reload changelog for build ${build.slice(0, 8)}`}
        aria-expanded={open}
        className="ml-auto flex h-7 cursor-pointer items-center gap-1 font-mono text-label-12 text-gray-900 hover:text-gray-1000"
      >
        {build.slice(0, 8)}
        <ChevronIcon open={open} />
      </button>
      <section
        id="reload-history"
        popover="auto"
        aria-labelledby="history-title"
        onToggle={(e) => setOpen(e.currentTarget.matches(":popover-open"))}
        className="fixed inset-auto top-12 left-edge m-0 w-[min(520px,calc(100vw-32px))] border border-gray-400 bg-background-100 text-gray-1000"
      >
        <div className="flex items-center justify-between border-b border-gray-400 px-edge py-2">
          <h2 id="history-title" className="text-heading-14">Reload changelog</h2>
          <button
            type="button"
            popoverTarget="reload-history"
            popoverTargetAction="hide"
            className="cursor-pointer px-2 py-1 text-label-12 text-gray-900 hover:bg-gray-100 hover:text-gray-1000"
          >
            Close
          </button>
        </div>
        {open && (view ? <History view={view} live={live} tzOffsetMin={tzOffsetMin} /> : (
          <p role="status" className="px-edge py-3 text-label-13 text-gray-700">{live ? "Loading updates…" : "Waiting for connection…"}</p>
        ))}
      </section>
    </>
  );
}
