import { useState } from "react";
import { RoomStream, type ChatAsks } from "./RoomStream";
import { SeatModelPanel, useSeatModel } from "./SeatModel";
import { sendCommand } from "./bridge";
import { coldMinutes } from "./format";
import { ContextRing } from "./ContextRing";
import { Cold } from "./icons";
import type { AgentLight, AgentRow, Color, PlaceView } from "./types";
import { Button } from "./ui/Button";
import { Composer } from "./ui/Composer";
import { Dot } from "./ui/Dot";

const LIGHT_COLOR: Record<AgentLight, Color> = {
  working: "green",
  asking: "grey",
  error: "red",
  waiting: "grey",
  idle: "grey",
};

/** Status dot for a seat switcher button or chat header: asking stays grey, working pulses green, error is red. */
export function SeatDot({ light }: { light: AgentLight }) {
  return <Dot color={LIGHT_COLOR[light]} pulse={light === "working"} />;
}

function cacheWords(seat: AgentRow | undefined, nowMs: number): { cold: number | null; text: string } {
  const cache = seat?.cache;
  if (cache === undefined) return { cold: null, text: "" };
  if (cache.is === "running") return { cold: null, text: "warm" };
  if (cache.cold_at_ms <= nowMs) return { cold: null, text: "cold" };
  const cold = coldMinutes(cache.cold_at_ms, nowMs);
  if (cold !== null) return { cold, text: "" };
  return { cold: null, text: `warm ${Math.ceil((cache.cold_at_ms - nowMs) / 60_000)}m` };
}

function usePlaceComposer(place: string, live: boolean) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const draft = drafts[place] ?? "";

  const setDraft = (value: string) => {
    setDrafts((prev) => ({ ...prev, [place]: value }));
    if (sendError !== null) setSendError(null);
  };

  const submit = (delivery?: "now") => {
    const text = draft.trim();
    if (!live || sending || place === "" || text === "") return;
    const target = place;
    setSending(true);
    setSendError(null);
    sendCommand({ say: { place: target, text, delivery } })
      .then(() => setDrafts((prev) => ({ ...prev, [target]: "" })))
      .catch((e: unknown) => setSendError(e instanceof Error ? e.message : String(e)))
      .finally(() => setSending(false));
  };

  return { draft, sending, sendError, setDraft, submit };
}

/** Right pane of the Card Room: open seat header, scrollable stream, model panel, and fused full-bleed composer. */
export function SeatChat({
  seat,
  place,
  name,
  room,
  view,
  answering,
  asks,
  tzOffsetMin,
  nowMs,
  live,
}: {
  seat: AgentRow | undefined;
  place: string;
  name: string;
  room: string;
  view: PlaceView | undefined;
  answering: boolean;
  asks: ChatAsks;
  tzOffsetMin: number;
  nowMs: number;
  live: boolean;
}) {
  const { draft, sending, sendError, setDraft, submit } = usePlaceComposer(place, live);
  const model = useSeatModel(seat, live);
  const cache = cacheWords(seat, nowMs);
  const hasModel = model.seat !== undefined && model.next !== undefined && model.choices.length > 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-gray-400 px-gutter py-3">
        {seat !== undefined && <SeatDot light={seat.light} />}
        <h2 className="font-mono text-label-14 font-medium text-gray-1000">{name}</h2>
        {room !== "" && <span className="text-label-12 text-gray-900">{room}</span>}
        {seat !== undefined && (
          <span className="ml-auto flex items-center gap-3 font-mono text-label-12 tabular-nums text-gray-900">
            <ContextRing context={seat.context} />
            {cache.cold !== null ? (
              <Cold minutes={cache.cold} />
            ) : (
              cache.text !== "" && <span className={cache.text === "cold" ? "text-gray-700" : undefined}>{cache.text}</span>
            )}
          </span>
        )}
      </div>
      {view !== undefined ? (
        <div className="flex min-h-0 flex-1">
          <RoomStream {...asks} key={view.place} view={view} tzOffsetMin={tzOffsetMin} nowMs={nowMs} live={live} />
        </div>
      ) : (
        <section className="flex min-h-0 flex-1 items-center justify-center p-gutter">
          <p className="text-copy-14 text-gray-700">Waiting for {name}…</p>
        </section>
      )}
      <SeatModelPanel m={model} />
      <Composer
        value={draft}
        onChange={setDraft}
        onSubmit={() => submit()}
        onSendNow={() => submit("now")}
        placeholder={answering ? `Answer ${name}…` : `Message ${name}…`}
        disabled={!live || place === ""}
        sending={sending}
        error={sendError}
        left={
          hasModel && model.next !== undefined ? (
            <Button
              tier="secondary"
              size="sm"
              aria-expanded={model.open}
              aria-label={`${name} model and effort`}
              onClick={model.toggle}
            >
              {model.next.model.label} · {model.next.effort}
            </Button>
          ) : undefined
        }
      />
    </div>
  );
}
