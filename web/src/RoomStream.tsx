import { memo, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { AskLabel, InlineAsk, keepLines } from "./InlineAsk";
import { Markdown } from "./Markdown";
import { ActivityTracker, EndedTurnRow } from "./Turns";
import { fetchImage, readHistory, sendCommand } from "./bridge";
import { authorName, clockTime, plural } from "./format";
import type { ChatTurn, HandView, ImageView, MessageView, PlaceView, ReplyHand, TriageView } from "./types";
import { useRetainedStream } from "./streamHistory";
import { Button } from "./ui/Button";
import { Dot } from "./ui/Dot";
import { ImageThumbnail } from "./ui/ImagePreview";
import { PinnedScroll } from "./ui/PinnedScroll";

type StreamEntry = { kind: "message"; m: MessageView; run?: MessageView[] } | { kind: "turn"; t: ChatTurn };

// Place ended turns before the first message with a greater seq, rest at the end.
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
      e.m.pending === undefined && last.m.pending === undefined &&
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

// A closed ask with its typed options and pick, as its hand gave them.
function askText(h: ReplyHand): string {
  const options = (h.options ?? []).map((o, i) => `${i + 1}. ${o}`);
  if (!h.ask || options.length === 0) return "";
  const named = h.pick !== undefined && h.pick >= 1 && h.pick <= options.length;
  const why = h.reason !== undefined && h.reason !== "" ? `: ${h.reason}` : ".";
  return `${options.join("\n")}${named ? `\n\nI'd pick ${h.pick}${why}` : ""}`;
}

function firstLine(text: string): string {
  return text.split("\n", 1)[0]?.trim() ?? "";
}

// Only keyed bridge reads can supply the preview. Unavailable files keep their filename in the conversation.
function Thumbnail({ image }: { image: ImageView }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    setSrc(null);
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
      .catch(() => { if (!gone) setSrc(null); });
    return () => {
      gone = true;
      if (made !== null) URL.revokeObjectURL(made);
    };
  }, [image.url]);
  const name = image.path.split("/").pop() ?? image.path;
  if (src === null) return <span className="text-label-12 text-gray-700">{name}</span>;
  return <ImageThumbnail url={src} name={name} />;
}

function MessageExtras({ m }: { m: MessageView }) {
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
  delivery,
  m,
  tzOffsetMin,
}: {
  label: string;
  preview: string;
  body: string;
  expandable: boolean;
  delivery: ReactNode;
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
    <div data-unfold-row className="py-2">
      <div className="flex items-center gap-3">
        {expandable ? (
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left text-label-12 text-gray-700 hover:text-gray-900"
          >
            {line}
          </button>
        ) : (
          <div className="flex min-w-0 flex-1 items-center gap-3 text-label-12 text-gray-700">{line}</div>
        )}
        {delivery}
      </div>
      {open && (
        <div className="mt-2 border-l border-gray-400 pl-3">
          {body !== "" && <Markdown text={body} className="text-gray-900" />}
          <MessageExtras m={m} />
        </div>
      )}
    </div>
  );
}

function MessageDelivery({ m, place, live }: { m: MessageView; place: string; live: boolean }) {
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const interrupt = () => {
    setSending(true);
    setError(null);
    sendCommand({ "message.send-now": { place, seq: m.seq } })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setSending(false));
  };
  return (
    <>
      {m.pending !== undefined && <span className="text-label-12 text-gray-900">Unread</span>}
      {m.pending && <Button tier="secondary" size="sm" disabled={!live || sending} onClick={interrupt}
        title="Interrupt the running turn to read pending messages">Send now</Button>}
      {error && <span role="alert" className="text-copy-13 text-red-900">{error}</span>}
    </>
  );
}

interface MessageRowProps {
  place: string;
  m: MessageView;
  reviewForeman?: string;
  tzOffsetMin: number;
  nowMs: number;
  live: boolean;
  hand?: HandView;
  triage?: TriageView;
  run?: MessageView[];
}

function MessageRow({ m, place, reviewForeman, tzOffsetMin, nowMs, live, hand, triage, run }: MessageRowProps) {
  const recipient = reviewForeman && (m.to === "foreman" || m.to === reviewForeman) ? "review foreman" : m.to;
  const delivery = <MessageDelivery m={m} place={place} live={live} />;
  const times = run?.length ?? 1;
  if (m.kind === "marker") {
    return (
      <FoldedRow
        delivery={delivery}
        label={m.to !== null ? `engine → ${recipient}` : "engine"}
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
  if (m.kind === "agent" && m.hand === null && m.to !== null && m.to !== "operator") {
    return (
      <FoldedRow
        delivery={delivery}
        label={`${authorName(m.from)} → ${recipient}`}
        preview={firstLine(m.headline !== "" ? m.headline : m.text)}
        body={m.text !== "" ? m.text : m.headline}
        expandable
        m={m}
        tzOffsetMin={tzOffsetMin}
      />
    );
  }
  const operator = m.kind === "operator";
  const engine = m.kind === "engine";
  let body = m.text || m.headline;
  if (hand?.kind === "approve-brief") {
    body = m.hand?.message ?? "";
  } else if (m.hand !== null) {
    const options = hand?.kind === "answer" ? "" : askText(m.hand);
    body = [m.text, options].filter((part) => part.trim() !== "").join("\n\n");
  } else if (hand?.choices) {
    body = hand.choices.ask;
  }
  if (hand?.kind === "answer") body = keepLines(body);
  const head = (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      {m.hand !== null && m.hand.color !== "yellow" && <Dot color={m.hand.color} />}
      <span className={`text-label-13 font-medium ${engine ? "text-gray-900" : "text-gray-1000"}`}>
        {authorName(m.from)}
      </span>
      {m.from.is === "agent" && m.from.label !== m.from.role && m.from.label !== "review foreman" && (
        <span className="text-label-12 text-gray-900">{m.from.role}</span>
      )}
      {hand !== undefined && <AskLabel hand={hand} nowMs={nowMs} />}
      <span className="font-mono text-label-12 tabular-nums text-gray-700">{clockTime(m.time_ms, tzOffsetMin)}</span>
      {delivery}
    </div>
  );
  if (operator) {
    return (
      <article id={hand?.ask} data-seq={m.seq} className="py-3">
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
    <article id={hand?.ask} data-seq={m.seq} className="py-3">
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
      {hand !== undefined && <InlineAsk key={hand.ask} hand={hand} live={live} triage={triage} />}
    </article>
  );
}

function useStreamHistory(view: PlaceView, live: boolean) {
  const { messages, earlier, turns, firstSeq, prepend } = useRetainedStream(view);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const prependHeightRef = useRef<number | null>(null);

  const loadEarlier = (el: HTMLDivElement | null) => {
    if (!live || loading || firstSeq === undefined) return;
    setLoading(true);
    setError(null);
    readHistory(view.place, firstSeq, true)
      .then((page) => {
        if (el !== null) prependHeightRef.current = el.scrollHeight;
        prepend(page);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  };

  return {
    messages,
    earlier,
    turns,
    loading,
    error,
    loadEarlier,
    prependHeightRef,
  };
}

export interface ChatAsks {
  hands: HandView[];
  triage?: TriageView;
  jump?: number;
}

interface StreamProps extends ChatAsks {
  view: PlaceView;
  tzOffsetMin: number;
  nowMs: number;
  live: boolean;
}

/** One seat's scrollable conversation stream, interleaved with ended turns and inline live activity trackers. */
export const RoomStream = memo(function RoomStream({ view, tzOffsetMin, nowMs, live, hands, triage, jump }: StreamProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const keepRef = useRef(false);
  const { messages, earlier, turns, loading, error, loadEarlier, prependHeightRef } = useStreamHistory(view, live);
  const reviewForeman = view.card?.mode === "review" ? `${view.card.id}/foreman` : undefined;
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

  const bySeq = new Map(hands.map((hand) => [hand.seq, hand]));
  const loaded = new Set(messages.map((m) => m.seq));
  const pinned = (view.pinned ?? []).filter((m) => bySeq.has(m.seq) && !loaded.has(m.seq));
  const row = (m: MessageView, run?: MessageView[]) => (
    <MessageRow place={view.place} reviewForeman={reviewForeman} key={m.seq} m={m} run={run} tzOffsetMin={tzOffsetMin} nowMs={nowMs}
      hand={bySeq.get(m.seq)} triage={triage} live={live} />
  );

  const reached = useRef<number | undefined>(undefined);
  useLayoutEffect(() => {
    if (jump === undefined) {
      reached.current = undefined;
      return;
    }
    if (jump === reached.current) return;
    const box = scrollRef.current;
    const target = box?.querySelector<HTMLElement>(`[data-seq="${jump}"]`);
    if (box && target) {
      keepRef.current = true;
      target.scrollIntoView({ block: "center" });
      reached.current = jump;
    } else if (earlier > 0 && messages.length > 0 && jump < messages[0].seq && !error && !loading) {
      loadEarlier(box);
    }
  }, [jump, earlier, messages, loading, error, loadEarlier]);

  return (
    <section className="flex min-w-0 flex-1 flex-col">
      <PinnedScroll scrollRef={scrollRef} keepRef={keepRef}>
        <div className="px-gutter py-2">
          {pinned.map((m) => row(m))}
          {earlier > 0 && messages.length > 0 && (
            <div className="py-2">
              <button
                type="button"
                disabled={!live || loading}
                onClick={() => loadEarlier(scrollRef.current)}
                className="h-control cursor-pointer text-label-12 text-gray-900 hover:text-gray-1000 hover:underline disabled:cursor-not-allowed disabled:opacity-50"
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
          {entries.length === 0 && pinned.length === 0 ? (
            <p className="py-4 text-label-13 text-gray-700">No messages yet.</p>
          ) : (
            entries.map((entry) =>
              entry.kind === "message" ? (
                row(entry.m, entry.run)
              ) : (
                <EndedTurnRow reviewForeman={reviewForeman} key={`t:${entry.t.turn.job}`} turn={entry.t} tzOffsetMin={tzOffsetMin} nowMs={nowMs} />
              ),
            )
          )}
          {running.map((t) => (
            <ActivityTracker reviewForeman={reviewForeman} key={t.turn.job} turn={t} nowMs={nowMs} />
          ))}
        </div>
      </PinnedScroll>
    </section>
  );
});
