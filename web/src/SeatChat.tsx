import { useRef, useState } from "react";
import { RoomStream, type ChatAsks } from "./RoomStream";
import { SeatModelPanel, useSeatModel } from "./SeatModel";
import { sendCommand, uploadImage } from "./bridge";
import { coldMinutes } from "./format";
import { ContextRing } from "./ContextRing";
import { Cold } from "./icons";
import type { AgentLight, AgentRow, Color, PlaceView } from "./types";
import { Button } from "./ui/Button";
import { Composer, type Attachment } from "./ui/Composer";
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

interface DraftImage extends Attachment {
  store: () => Promise<string>;
}

interface Draft {
  text: string;
  images: DraftImage[];
  sending?: boolean;
  error?: string;
  failedImage?: number;
}

const EMPTY_DRAFT: Draft = { text: "", images: [] };

function attachment(file: File, id: number): DraftImage {
  let upload: Promise<string> | undefined;
  return {
    file,
    id,
    store() {
      // Send can join a paste in flight or reuse its path. A refused upload can be retried.
      upload ??= uploadImage(file).catch((error: unknown) => {
        upload = undefined;
        throw error;
      });
      return upload;
    },
  };
}

function usePlaceComposer(place: string, live: boolean) {
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const nextId = useRef(0);
  const draft = drafts[place] ?? EMPTY_DRAFT;
  const update = (apply: (previous: Draft) => Draft) => {
    setDrafts((all) => ({ ...all, [place]: apply(all[place] ?? EMPTY_DRAFT) }));
  };
  const setText = (text: string) => update((prev) => ({ ...prev, text, error: undefined }));
  const remove = (id: number) => update((prev) => ({
    ...prev, images: prev.images.filter((item) => item.id !== id),
    error: prev.failedImage === id ? undefined : prev.error,
  }));
  const paste = (files: File[]) => {
    const added = files.map((file) => attachment(file, nextId.current++));
    update((prev) => ({ ...prev, images: [...prev.images, ...added], error: undefined }));
    for (const item of added) {
      item.store().catch((error: Error) => update((prev) => prev.images.includes(item)
        ? { ...prev, failedImage: item.id, error: error.message } : prev));
    }
  };
  const submit = async (delivery?: "now") => {
    const text = draft.text.trim();
    if (!live || draft.sending || !place || (!text && !draft.images.length)) return;
    update((prev) => ({ ...prev, sending: true, error: undefined, failedImage: undefined }));
    try {
      const images = await Promise.all(draft.images.map((item) => item.store()));
      await sendCommand({ say: { place, text, delivery, images: images.length ? images : undefined } });
      update(() => EMPTY_DRAFT);
    } catch (error) {
      update((prev) => ({ ...prev, sending: false, error: error instanceof Error ? error.message : String(error) }));
    }
  };

  return { draft, setText, remove, paste, submit };
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
  const { draft, setText, remove, paste, submit } = usePlaceComposer(place, live);
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
