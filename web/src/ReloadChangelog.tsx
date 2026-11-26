import { useEffect, useRef, useState } from "react";
import { readHistory, watchMain } from "./bridge";
import { clockTime } from "./format";
import { ChevronIcon } from "./icons";
import type { HistoryPage, MessageView, PlaceView } from "./types";

const DATE = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });

function updates(messages: MessageView[]): MessageView[] {
  return messages.filter((m) =>
    m.from.is === "engine" && m.kind === "engine" && m.to === "operator" &&
    /^(Reloaded to |The reload failed:|The reload to |The build of )/.test(m.text)
  ).reverse();
}

// The cursor follows all main-room messages, including pages without a matching update.
function olderCursor(page: HistoryPage): number | null {
  return page.earlier === 0 ? null : page.messages[0]?.seq ?? Number.MAX_SAFE_INTEGER;
}

function fromTail(view: PlaceView) {
  return { entries: updates(view.messages), before: olderCursor(view), total: view.earlier + view.messages.length };
}

interface HistoryProps {
  view: PlaceView;
  live: boolean;
  tzOffsetMin: number;
}

function History({ view, live, tzOffsetMin }: HistoryProps) {
  const [history, setHistory] = useState(() => fromTail(view));
  const [error, setError] = useState<string | null>(null);
  const total = view.earlier + view.messages.length;
  if (total !== history.total) {
    if (history.total < view.earlier || total < history.total || history.before === Number.MAX_SAFE_INTEGER) {
      // After a gap the latest tail is contiguous; its cursor can recover everything before it.
      setHistory(fromTail(view));
    } else {
      const added = updates(view.messages.slice(history.total - view.earlier));
      setHistory({ ...history, total, entries: [...added, ...history.entries] });
    }
  }

  const bottom = useRef<HTMLDivElement>(null);
  const before = history.before;
  useEffect(() => {
    const target = bottom.current;
    if (!live || !target || before === null || error !== null) return;
    let active = true;
    const observer = new IntersectionObserver(([entry]) => {
      if (!active || !entry.isIntersecting) return;
      observer.disconnect();
      readHistory("main", before).then((page) => {
        if (!active) return;
        const next = olderCursor(page);
        if (next !== null && next >= before) throw new Error("Could not read earlier main-room messages.");
        const older = updates(page.messages);
        setHistory((current) => current.before !== before ? current : {
          ...current, before: next, entries: [...current.entries, ...older],
        });
      }).catch((e: unknown) => {
        if (active) setError(e instanceof Error ? e.message : String(e));
      });
    }, { root: target.parentElement });
    observer.observe(target);
    return () => {
      active = false;
      observer.disconnect();
    };
  }, [before, error, live]);

  return (
    <div className="max-h-[min(28rem,65vh)] overflow-y-auto">
      <ol className="divide-y divide-gray-400">
        {history.entries.map((m) => (
          <li key={m.seq} className="px-edge py-3">
            <time dateTime={new Date(m.time_ms).toISOString()} className="mb-1 block font-mono text-label-12 tabular-nums text-gray-700">
              {DATE.format(m.time_ms + tzOffsetMin * 60_000)}, {clockTime(m.time_ms, tzOffsetMin)}
            </time>
            <div className="whitespace-pre-wrap wrap-anywhere text-copy-14">{m.text}{m.cut && "…"}</div>
          </li>
        ))}
      </ol>
      {before === null && history.entries.length === 0 && (
        <p className="px-edge py-3 text-label-13 text-gray-700">No reloads or build updates yet.</p>
      )}
      {before !== null && error === null && (
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
