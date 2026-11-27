import { useLayoutEffect, useRef, useState } from "react";
import { RoomStream, type ChatAsks } from "./RoomStream";
import { SeatModelPanel, useSeatModel } from "./SeatModel";
import { CommandHelp } from "./CommandHelp";
import { usePlaceComposer } from "./chatDraft";
import { cardPath, navigate } from "./route";
import { coldMinutes } from "./format";
import { ContextRing } from "./ContextRing";
import { Cold } from "./icons";
import type { AgentLight, AgentRow, Color, ComposerAction, PlaceView } from "./types";
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
  const [help, setHelp] = useState<{ place: string; commands: [string, string][] } | null>(null);
  const activePlace = useRef<string | null>(place);
  useLayoutEffect(() => {
    activePlace.current = place;
    setHelp(null);
    return () => { activePlace.current = null; };
  }, [place]);
  const show = (action: ComposerAction) => {
    if (activePlace.current !== place) return;
    if (action.action === "help") setHelp({ place, commands: action.commands });
    else {
      setHelp(null);
      navigate(action.action === "usage" ? "/usage" : cardPath(action.card));
    }
  };
  const { draft, setText, remove, paste, submit } = usePlaceComposer(place, view, live, show);
  const commands = help?.place === place ? help.commands : undefined;
  const model = useSeatModel(seat, live);
  const cache = cacheWords(seat, nowMs);
  const hasModel = model.seat !== undefined && model.next !== undefined && model.choices.length > 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-gray-400 px-gutter py-3">
        {seat !== undefined && <SeatDot light={seat.light} />}
        <h2 className={`min-w-0 text-label-14 font-medium text-gray-1000 ${seat?.topic ? "[overflow-wrap:anywhere]" : "font-mono"}`}>
          {seat?.topic || name}
        </h2>
        {room !== "" && <span className="text-label-12 text-gray-900">{room}</span>}
        {seat !== undefined && (
          <span className="ml-auto flex shrink-0 items-center gap-3 font-mono text-label-12 tabular-nums text-gray-900">
            <ContextRing context={seat.context} />
            {cache.cold !== null ? (
              <Cold minutes={cache.cold} />
            ) : (
              cache.text !== "" && <span className={cache.text === "cold" ? "text-gray-700" : undefined}>{cache.text}</span>
            )}
          </span>
        )}
      </div>
      {commands && <CommandHelp commands={commands} close={() => setHelp(null)} />}
      {view !== undefined ? (
        <div className={commands ? "hidden" : "flex min-h-0 flex-1"}>
          <RoomStream {...asks} key={view.place} view={view} tzOffsetMin={tzOffsetMin} nowMs={nowMs} live={live} />
        </div>
      ) : !commands && (
        <section className="flex min-h-0 flex-1 items-center justify-center p-gutter">
          <p className="text-copy-14 text-gray-700">Waiting for {name}…</p>
        </section>
      )}
      <SeatModelPanel m={model} />
      <Composer
        value={draft.text}
        onChange={setText}
        attachments={draft.images}
        onPasteImages={paste}
        onRemove={remove}
        onSubmit={() => submit()}
        onSendNow={() => submit("now")}
        placeholder={answering ? `Answer ${name}…` : `Message ${name}…`}
        disabled={!live || place === ""}
        sending={draft.sending}
        error={draft.error}
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
