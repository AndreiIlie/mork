import { useEffect, useRef, useState } from "react";
import { readHistory, watchMain } from "./bridge";
import { clockTime } from "./format";
import { List } from "./ui/List";
import { SectionHead } from "./ui/Section";
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
    });
    observer.observe(target);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [error, firstSeq, hasEarlier, live, prepend]);

  return (
    <>
      <ol className="divide-y divide-gray-400">
        {entries.map((m) => (
          <li key={m.seq} className="flex items-center gap-3 px-edge py-3">
            <time dateTime={new Date(m.time_ms).toISOString()} className="w-[112px] shrink-0 font-mono text-label-12 tabular-nums text-gray-700">
              {DATE.format(m.time_ms + tzOffsetMin * 60_000)}, {clockTime(m.time_ms, tzOffsetMin)}
            </time>
            <div className="min-w-0 flex-1 whitespace-pre-wrap wrap-anywhere text-copy-14">{m.text}{m.cut && "…"}</div>
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
    </>
  );
}

export function ReloadChangelog({ view, live, tzOffsetMin }: Omit<HistoryProps, "view"> & { view?: PlaceView }) {
  useEffect(() => {
    watchMain(true);
    return () => watchMain(false);
  }, []);

  return (
    <section className="mt-gutter">
      <SectionHead title="Reload changelog" />
      <List>
        {view ? <History view={view} live={live} tzOffsetMin={tzOffsetMin} /> : (
          <p role="status" className="px-edge py-3 text-label-13 text-gray-700">{live ? "Loading updates…" : "Waiting for connection…"}</p>
        )}
      </List>
    </section>
  );
}
