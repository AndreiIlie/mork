import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Markdown } from "./Markdown";
import { ActivityTracker, EndedTurnRow } from "./Turns";
import { fetchImage, requestCommand } from "./bridge";
import { authorName, clockTime, plural } from "./format";
import type { ChatTurn, HistoryPage, ImageView, MessageView, PlaceView, ReplyHand } from "./types";
import { Dot } from "./ui/Dot";
import { PinnedScroll } from "./ui/PinnedScroll";

const HISTORY_COUNT = 100;

interface RetainedStream {
  messages: MessageView[];
  earlier: number;
  turns: ChatTurn[];
}

type StreamEntry = { kind: "message"; m: MessageView; run?: MessageView[] } | { kind: "turn"; t: ChatTurn };

function isHistoryAnswer(value: unknown): value is { history: HistoryPage } {
  if (typeof value !== "object" || value === null) return false;
  const h = (value as { history?: Partial<HistoryPage> }).history;
  return h !== undefined && typeof h === "object" && h !== null && Array.isArray(h.messages) && typeof h.earlier === "number";
}

function prependHistory(prev: RetainedStream, page: HistoryPage): RetainedStream {
  const bySeq = new Map(prev.messages.map((m) => [m.seq, m]));
  for (const m of page.messages) {
    if (!bySeq.has(m.seq)) bySeq.set(m.seq, m);
  }
  const byJob = new Map(prev.turns.map((t) => [t.turn.job, t]));
  for (const t of page.turns ?? []) {
    if (!byJob.has(t.turn.job)) byJob.set(t.turn.job, t);
  }
  const messages = [...bySeq.values()].sort((a, b) => a.seq - b.seq);
  return { messages, earlier: Math.min(prev.earlier, page.earlier), turns: [...byJob.values()] };
}

// Keep messages and ended turns that scroll off the moving tail while open, letting live turns supersede stale running state.
function reconcileTail(prev: RetainedStream, view: PlaceView): RetainedStream {
  const liveTurns = view.turns ?? [];
  if (prev.messages.length === 0 || prev.earlier + prev.messages.length < view.earlier) {
    return { messages: view.messages, earlier: view.earlier, turns: liveTurns };
  }
  const bySeq = new Map(prev.messages.map((m) => [m.seq, m]));
  for (const m of view.messages) bySeq.set(m.seq, m);
  const byJob = new Map(prev.turns.filter((t) => !t.running).map((t) => [t.turn.job, t]));
  for (const t of liveTurns) byJob.set(t.turn.job, t);
  const messages = [...bySeq.values()].sort((a, b) => a.seq - b.seq);
  const firstView = view.messages[0]?.seq;
  const earlier = firstView !== undefined && firstView <= prev.messages[0].seq ? view.earlier : prev.earlier;
  return { messages, earlier, turns: [...byJob.values()] };
}

// Matches crates/ui/src/activity.rs `entries`: ended turns before the first message with a greater seq, rest at the end.
function interleaveEntries(messages: MessageView[], turns: ChatTurn[]): StreamEntry[] {
  const ended = turns.filter((t) => !t.running && t.step_count > 0).sort((a, b) => a.after_seq - b.after_seq);
  const out: StreamEntry[] = [];
  let ti = 0;
  for (const m of messages) {
    while (ti < ended.length && ended[ti].after_seq < m.seq) {
      out.push({ kind: "turn", t: ended[ti++] });
    }
    out.push({ kind: "message", m });
  }
  while (ti < ended.length) {
    out.push({ kind: "turn", t: ended[ti++] });
  }
  return collapseRepeats(out);
}

// Consecutive engine lines saying the same thing to the same place show once, with the latest's time and a count.
function collapseRepeats(entries: StreamEntry[]): StreamEntry[] {
  const out: StreamEntry[] = [];
  for (const e of entries) {
    const last = out[out.length - 1];
    if (
      e.kind === "message" &&
      e.m.kind === "marker" &&
      last !== undefined &&
      last.kind === "message" &&
      last.m.kind === "marker" &&
      e.m.headline.trim() !== "" &&
      last.m.to === e.m.to &&
      last.m.headline === e.m.headline
    ) {
      out[out.length - 1] = { kind: "message", m: e.m, run: [...(last.run ?? [last.m]), e.m] };
    } else {
      out.push(e);
    }
  }
  return out;
}

// A closed ask with its typed options and pick, as its hand gave them (fac-696).
function askText(h: ReplyHand): string {
  const options = (h.options ?? []).map((o, i) => `${i + 1}. ${o}`);
  if (options.length === 0 || h.ask.trim() === "") return h.ask;
  const named = h.pick !== undefined && h.pick >= 1 && h.pick <= options.length;
  const why = h.reason !== undefined && h.reason !== "" ? `: ${h.reason}` : ".";
  return `${h.ask}\n\n${options.join("\n")}${named ? `\n\nI'd pick ${h.pick}${why}` : ""}`;
}

function firstLine(text: string): string {
  return text.split("\n", 1)[0]?.trim() ?? "";
}

// A published image loads with the bridge's key; a click opens it full size. One the bridge doesn't serve shows its name.
function Thumbnail({ image }: { image: ImageView }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (image.url === undefined) return undefined;
    let made: string | null = null;
    let gone = false;
    fetchImage(image.url)
      .then((u) => {
        if (gone) URL.revokeObjectURL(u);
        else {
          made = u;
          setSrc(u);
        }
      })
      .catch(() => setSrc(null));
    return () => {
      gone = true;
      if (made !== null) URL.revokeObjectURL(made);
    };
  }, [image.url]);
  const name = image.path.split("/").pop() ?? image.path;
  if (src === null) return <span className="text-label-12 text-gray-700">{name}</span>;
  return (
    <a href={src} target="_blank" rel="noreferrer" title={name}>
      <img src={src} alt={name} className="h-24 w-auto max-w-full border border-gray-400" />
    </a>
  );
}

export function MessageExtras({ m }: { m: MessageView }) {
  const images = m.images ?? [];
  return (
    <>
      {images.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-2">
          {images.map((image, i) => (
            <Thumbnail key={`${image.path}:${i}`} image={image} />
          ))}
        </div>
      )}
      {m.cut && <p className="mt-1 text-label-12 text-gray-700">cut</p>}
      {m.findings.length > 0 && (
        <ul className="mt-2 max-w-[74ch] divide-y divide-gray-400 border-y border-gray-400">
          {m.findings.map((f, i) => (
            <li key={`${f.file}:${f.line}:${i}`} className="flex gap-3 py-1.5 text-label-13">
              <span className="shrink-0 font-mono text-label-12 text-gray-900">
                {f.file}:{f.line}
              </span>
              <span className="min-w-0 text-gray-1000">{f.summary}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function FoldedRow({
  label,
  preview,
  body,
  expandable,
  m,
  tzOffsetMin,
}: {
  label: string;
  preview: string;
  body: string;
  expandable: boolean;
  m: MessageView;
  tzOffsetMin: number;
}) {
  const [open, setOpen] = useState(false);
  const line = (
    <>
      <span className="shrink-0 font-mono">{label}</span>
      {preview !== "" && <span className="min-w-0 truncate">{preview}</span>}
      <span className="h-px min-w-6 flex-1 bg-gray-400" />
      <span className="shrink-0 font-mono tabular-nums">{clockTime(m.time_ms, tzOffsetMin)}</span>
    </>
  );
  return (
    <div className="py-2">
      {expandable ? (
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          className="flex w-full cursor-pointer items-center gap-3 text-left text-label-12 text-gray-700 hover:text-gray-900"
        >
          {line}
        </button>
      ) : (
        <div className="flex items-center gap-3 text-label-12 text-gray-700">{line}</div>
      )}
      {open && (
        <div className="mt-2 border-l border-gray-400 pl-3">
          {body !== "" && <Markdown text={body} className="text-gray-900" />}
          <MessageExtras m={m} />
        </div>
      )}
    </div>
  );
}

/** One stream message row, optionally tagged with its room for the per-card Overview. */
export function MessageRow({
  m,
  tzOffsetMin,
  run,
  room,
}: {
  m: MessageView;
  tzOffsetMin: number;
  run?: MessageView[];
  room?: string;
}) {
  const times = run?.length ?? 1;
  if (m.kind === "marker") {
    return (
      <FoldedRow
        label={m.to !== null ? `engine → ${m.to}` : "engine"}
        preview={times > 1 ? `${firstLine(m.headline)} ×${times}` : firstLine(m.headline)}
        body={
          run !== undefined
            ? run
                .map((x) => `${clockTime(x.time_ms, tzOffsetMin)}  ${x.text.trim() !== "" ? x.text : x.headline}`)
                .join("\n\n")
            : m.text
        }
        expandable={run !== undefined || m.text.trim() !== ""}
        m={m}
        tzOffsetMin={tzOffsetMin}
      />
    );
  }
  if (m.kind === "agent" && m.hand === null && m.to !== null && m.to !== "andrei") {
    return (
      <FoldedRow
        label={`${authorName(m.from)} → ${m.to}`}
        preview={firstLine(m.headline !== "" ? m.headline : m.text)}
        body={m.text !== "" ? m.text : m.headline}
        expandable
        m={m}
        tzOffsetMin={tzOffsetMin}
      />
    );
  }
  const andrei = m.kind === "andrei";
  const engine = m.kind === "engine";
  const body =
    m.hand !== null
      ? [m.text, m.ask_open ? "" : askText(m.hand)].filter((part) => part.trim() !== "").join("\n\n")
      : m.text !== ""
        ? m.text
        : m.headline;
  const head = (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      {m.hand !== null && m.hand.color !== "yellow" && <Dot color={m.hand.color} />}
      <span className={`text-label-13 font-medium ${engine ? "text-gray-900" : "text-gray-1000"}`}>
        {authorName(m.from)}
      </span>
      {room !== undefined ? (
        <span className="text-label-12 text-gray-900">{andrei ? `in ${room}` : room}</span>
      ) : (
        m.from.is === "agent" &&
        m.from.label !== m.from.role && <span className="text-label-12 text-gray-900">{m.from.role}</span>
      )}
      {m.ask_open && <span className="text-label-12 text-gray-900">asks below</span>}
      <span className="font-mono text-label-12 tabular-nums text-gray-700">{clockTime(m.time_ms, tzOffsetMin)}</span>
    </div>
  );
  if (andrei) {
    return (
      <article className="py-3">
        <div className="w-fit max-w-full border border-gray-400 bg-gray-100 px-3 py-2">
          {head}
          {body !== "" && (
            <div className="mt-1">
              <Markdown text={body} className="text-gray-1000" elevated />
            </div>
          )}
          <MessageExtras m={m} />
        </div>
      </article>
    );
  }
  return (
    <article className="py-3">
      {head}
      {m.hand !== null && m.hand.headline !== "" && (
        <p className="mt-1 text-label-14 font-medium text-gray-1000">{m.hand.headline}</p>
      )}
      {body !== "" && (
        <div className="mt-1">
          <Markdown text={body} className={m.hand !== null ? "text-gray-1000" : "text-gray-900"} />
        </div>
      )}
      <MessageExtras m={m} />
    </article>
  );
}

function useStreamHistory(view: PlaceView, live: boolean) {
  const [lastView, setLastView] = useState(view);
  const [stream, setStream] = useState<RetainedStream>(() => ({
    messages: view.messages,
    earlier: view.earlier,
    turns: view.turns ?? [],
  }));
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const prependHeightRef = useRef<number | null>(null);

  if (lastView !== view) {
    setLastView(view);
    setStream((prev) => reconcileTail(prev, view));
  }

  const loadEarlier = (el: HTMLDivElement | null) => {
    const firstSeq = stream.messages[0]?.seq;
    if (!live || loading || firstSeq === undefined) return;
    setLoading(true);
    setError(null);
    requestCommand({ history: { place: view.place, before: firstSeq, count: HISTORY_COUNT, turns: true } })
      .then((ok) => {
        if (!isHistoryAnswer(ok)) throw new Error("unexpected history response");
        if (el !== null) prependHeightRef.current = el.scrollHeight;
        setStream((prev) => prependHistory(prev, ok.history));
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  };

  return {
    messages: stream.messages,
    earlier: stream.earlier,
    turns: stream.turns,
    loading,
    error,
    loadEarlier,
    prependHeightRef,
  };
}

interface StreamProps {
  view: PlaceView;
  tzOffsetMin: number;
  nowMs: number;
  live: boolean;
}

/** One seat's scrollable conversation stream, interleaved with ended turns and inline live activity trackers. */
export const RoomStream = memo(function RoomStream({ view, tzOffsetMin, nowMs, live }: StreamProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const keepRef = useRef(false);
  const { messages, earlier, turns, loading, error, loadEarlier, prependHeightRef } = useStreamHistory(view, live);
  const entries = interleaveEntries(messages, turns);
  const running = turns.filter((t) => t.running);

  // Earlier messages land above the reader: keep the rows they were reading in place.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    const before = prependHeightRef.current;
    if (el === null || before === null) return;
    prependHeightRef.current = null;
    const added = el.scrollHeight - before;
    if (added <= 0) return;
    el.scrollTop += added;
    keepRef.current = true;
  }, [messages, prependHeightRef]);

  return (
    <section className="flex min-w-0 flex-1 flex-col">
      <PinnedScroll scrollRef={scrollRef} keepRef={keepRef}>
        <div className="px-gutter py-2">
          {earlier > 0 && messages.length > 0 && (
            <div className="py-2">
              <button
                type="button"
                disabled={!live || loading}
                onClick={() => loadEarlier(scrollRef.current)}
                className="cursor-pointer text-label-12 text-gray-900 hover:text-gray-1000 hover:underline disabled:cursor-not-allowed disabled:opacity-50"
              >
                {loading ? "Loading earlier messages…" : `${earlier} earlier ${plural(earlier, "message", "messages")}`}
              </button>
              {error !== null && (
                <p role="alert" className="mt-1 text-copy-13 text-red-900">
                  {error}
                </p>
              )}
            </div>
          )}
          {entries.length === 0 ? (
            <p className="py-4 text-label-13 text-gray-700">No messages yet.</p>
          ) : (
            entries.map((entry) =>
              entry.kind === "message" ? (
                <MessageRow key={`m:${entry.m.seq}`} m={entry.m} tzOffsetMin={tzOffsetMin} run={entry.run} />
              ) : (
                <EndedTurnRow key={`t:${entry.t.turn.job}`} turn={entry.t} tzOffsetMin={tzOffsetMin} nowMs={nowMs} />
              ),
            )
          )}
          {running.map((t) => (
            <ActivityTracker key={t.turn.job} turn={t} nowMs={nowMs} />
          ))}
        </div>
      </PinnedScroll>
    </section>
  );
});
