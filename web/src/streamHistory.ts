import { useCallback, useMemo, useState } from "react";
import type { ChatTurn, HistoryPage, MessageView, PlaceView } from "./types";

interface RetainedStream {
  messages: MessageView[];
  earlier: number;
  turns: ChatTurn[];
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

export function useRetainedStream(view: PlaceView) {
  const [lastView, setLastView] = useState(view);
  const [stream, setStream] = useState<RetainedStream>(() => ({
    messages: view.messages,
    earlier: view.earlier,
    turns: view.turns ?? [],
  }));

  if (lastView !== view) {
    setLastView(view);
    setStream((prev) => reconcileTail(prev, view));
  }

  const firstSeq = stream.messages[0]?.seq;
  const prepend = useCallback((page: HistoryPage) => {
    // A newer tail may have exposed a gap while this page was in flight.
    setStream((prev) => prev.messages[0]?.seq === firstSeq ? prependHistory(prev, page) : prev);
  }, [firstSeq]);

  // A loaded row outside the live tail can be read or returned to news while it stays onscreen.
  const messages = useMemo(() => {
    const tail = view.messages[0]?.seq ?? Infinity;
    if (stream.messages.length === 0 || stream.messages[0].seq >= tail) return stream.messages;
    const unread = new Map(view.unread);
    return stream.messages.map(m => {
      if (m.seq >= tail) return m;
      const pending = unread.get(m.seq);
      return pending === m.pending ? m : { ...m, pending };
    });
  }, [stream.messages, view]);
  return { ...stream, messages, firstSeq, prepend };
}
