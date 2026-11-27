import { useSyncExternalStore } from "react";
import { submitComposer, uploadImage } from "./bridge";
import type { ComposerAction, PlaceView } from "./types";
import type { Attachment } from "./ui/Composer";

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

// Navigation commands can unmount the room while its unsent images still belong to its draft.
const drafts = new Map<string, Draft>();
const subscribers = new Set<() => void>();
let nextImageId = 0;

function subscribe(refresh: () => void) {
  subscribers.add(refresh);
  return () => { subscribers.delete(refresh); };
}

export function usePlaceComposer(place: string, view: PlaceView | undefined, live: boolean, show: (action: ComposerAction) => void) {
  const current = () => drafts.get(place) ?? EMPTY_DRAFT;
  const draft = useSyncExternalStore(subscribe, current);
  const update = (apply: (previous: Draft) => Draft) => {
    const next = apply(current());
    if (!next.text && !next.images.length && !next.error && !next.sending) drafts.delete(place);
    else drafts.set(place, next);
    for (const refresh of subscribers) refresh();
  };
  const setText = (text: string) => update((prev) => ({ ...prev, text, error: undefined }));
  const remove = (id: number) => update((prev) => ({
    ...prev, images: prev.images.filter((item) => item.id !== id),
    error: prev.failedImage === id ? undefined : prev.error,
  }));
  const paste = (files: File[]) => {
    const added = files.map((file) => attachment(file, nextImageId++));
    update((prev) => ({ ...prev, images: [...prev.images, ...added], error: undefined }));
    for (const item of added) {
      item.store().catch((error: Error) => update((prev) => prev.images.includes(item)
        ? { ...prev, failedImage: item.id, error: error.message } : prev));
    }
  };
  const submit = async (delivery?: "now") => {
    const draft = current();
    const text = draft.text;
    if (!live || draft.sending || !place || (!text.trim() && !draft.images.length)) return;
    update((prev) => ({ ...prev, sending: true, error: undefined, failedImage: undefined }));
    try {
      const slash = /^\p{White_Space}*\//u.test(text);
      const images = slash ? [] : await Promise.all(draft.images.map((item) => item.store()));
      const card = view ? ("card" in view.kind ? view.kind.card : null) : place.split("/")[0];
      const context = { place, card, to: view?.input_to ?? "" };
      const { action } = await submitComposer({ text, context, images, now: delivery === "now" });
      update(prev => slash ? { ...prev, text: "", sending: false } : EMPTY_DRAFT);
      if (action) show(action);
    } catch (error) {
      update((prev) => ({ ...prev, sending: false, error: error instanceof Error ? error.message : String(error) }));
    }
  };

  return { draft, setText, remove, paste, submit };
}
