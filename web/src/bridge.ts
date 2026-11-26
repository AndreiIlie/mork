import type { ErrorBody, Frame, PlaceView, Screen } from "./types";

/** Where the link to the engine stands: `reconnecting` keeps the last screen it had. */
export type Link = "connecting" | "live" | "reconnecting" | "disconnected";

export interface BridgeState {
  link: Link;
  build: string | null;
  /** The engine's words while it answers `recovering`, before any screen. */
  recovering: string | null;
  screen: Screen | null;
  /** The browser's clock when `screen` arrived, to tick its countdowns from `now_ms`. */
  screenAt: number;
  /** Latest watched place views by place id; unwatched places are dropped. */
  views: Record<string, PlaceView>;
}

export const INITIAL_STATE: BridgeState = {
  link: "connecting",
  build: null,
  recovering: null,
  screen: null,
  screenAt: 0,
  views: {},
};

const KEY_STORAGE = "mork.web.key";
const KEY_PATTERN = /^[0-9a-fA-F]{64}$/;
const RETRY_MS = 2000;
const PROBE_MS = 500;
const WATCH_TAIL = 100;
const ALLOWED_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);
// The window's wiring: 0 probes status, 1 watches the screen and open card, commands count from 2.
const PROBE_ID = 0;
const WATCH_ID = 1;
const FIRST_COMMAND_ID = 2;
let memoryKey: string | null = null;
let liveDial: Dial | null = null;
let activeDial: Dial | null = null;
let activeUpdate: Update | null = null;
let watchedCardId: string | null = null;
let watchedExtraPlace: string | null = null;

function watchedPlaces(cardId: string | null, extraPlace: string | null): string[] {
  const places = cardId === null ? ["screen"] : ["screen", cardId, `${cardId}/spec`];
  if (extraPlace !== null && !places.includes(extraPlace)) places.push(extraPlace);
  return places;
}

function watchCommand(cardId: string | null, extraPlace: string | null): unknown {
  const places = watchedPlaces(cardId, extraPlace);
  if (places.length === 1) return { watch: { places } };
  return { watch: { places, tail: cardId === null ? 0 : WATCH_TAIL, full: true } };
}

function keepWatched(
  views: Record<string, PlaceView>,
  cardId: string | null,
  extraPlace: string | null,
): Record<string, PlaceView> {
  const allowed = new Set(watchedPlaces(cardId, extraPlace));
  const next: Record<string, PlaceView> = {};
  for (const [place, view] of Object.entries(views)) {
    if (allowed.has(place)) next[place] = view;
  }
  return next;
}

function validKey(value: string | null): string | null {
  return value !== null && KEY_PATTERN.test(value) ? value : null;
}

function isAllowedHost(): boolean {
  return ALLOWED_HOSTS.has(window.location.hostname.toLowerCase());
}

function takeFragmentKey(): string | null {
  const hash = window.location.hash;
  if (!hash.startsWith("#")) return null;
  const params = new URLSearchParams(hash.slice(1));
  if (!params.has("key")) return null;
  const key = validKey(params.get("key"));
  // Always strip #key= from the address, whatever the host, so the secret never lingers in history.
  window.history.replaceState(null, "", window.location.pathname + window.location.search);
  return key;
}

function storeKey(key: string): void {
  memoryKey = key;
  try {
    window.localStorage.setItem(KEY_STORAGE, key);
  } catch {
    // Ignore storage errors and keep the fragment key in memory for this session.
  }
}

function takeAndStoreFragmentKey(): string | null {
  const key = takeFragmentKey();
  // Only bridge-allowed hosts store or send the key; previews (<card>.localhost) stay disconnected.
  if (key === null || !isAllowedHost()) return null;
  storeKey(key);
  return key;
}

function resolveKey(): string | null {
  const fresh = takeAndStoreFragmentKey();
  if (fresh !== null) return fresh;
  if (!isAllowedHost()) return null;
  try {
    const stored = validKey(window.localStorage.getItem(KEY_STORAGE));
    if (stored !== null) return stored;
  } catch {
    // Fall back to the in-memory key if storage is unavailable.
  }
  return memoryKey;
}

function parseFrame(data: unknown): Frame | null {
  if (typeof data !== "string") return null;
  try {
    const parsed: unknown = JSON.parse(data);
    return typeof parsed === "object" && parsed !== null ? (parsed as Frame) : null;
  } catch {
    return null;
  }
}

function isScreen(value: unknown): value is Screen {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Partial<Screen>;
  return typeof s.now_ms === "number" && Array.isArray(s.hands) && Array.isArray(s.boards);
}

function isPlaceView(value: unknown): value is PlaceView {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Partial<PlaceView>;
  return typeof v.place === "string" && Array.isArray(v.messages) && typeof v.earlier === "number";
}

interface Pending {
  resolve: (ok: unknown) => void;
  reject: (error: Error) => void;
}

type Update = (next: Partial<BridgeState> | ((state: BridgeState) => Partial<BridgeState>)) => void;

class Dial {
  hadHello = false;
  private nextId = FIRST_COMMAND_ID;
  private pending = new Map<number, Pending>();
  private probeTimer: number | null = null;
  private readonly ws: WebSocket;
  private readonly update: Update;

  constructor(ws: WebSocket, update: Update) {
    this.ws = ws;
    this.update = update;
  }

  receive(data: unknown): void {
    const frame = parseFrame(data);
    if (frame === null) return;
    if ("hello" in frame) {
      if (typeof frame.hello?.build !== "string") return;
      this.hadHello = true;
      const build = frame.hello.build;
      // A dial that failed before its hello left `disconnected`; this one is through.
      this.update((s) => ({ build, link: s.screen === null ? "connecting" : "reconnecting" }));
      this.write(PROBE_ID, { status: {} });
    } else if ("event" in frame) {
      if (isScreen(frame.event?.screen)) {
        liveDial = this;
        this.update({ link: "live", recovering: null, screen: frame.event.screen, screenAt: Date.now() });
      } else if (isPlaceView(frame.event?.view)) {
        const view = frame.event.view;
        if (!watchedPlaces(watchedCardId, watchedExtraPlace).includes(view.place)) return;
        liveDial = this;
        this.update((s) => ({ views: { ...s.views, [view.place]: view } }));
      }
    } else if (frame.id === PROBE_ID || frame.id === WATCH_ID) {
      this.onSetup(frame.id, "err" in frame ? frame.err : null);
    } else {
      this.settle(frame.id, "err" in frame ? frame.err : null, "ok" in frame ? frame.ok : undefined);
    }
  }

  rewatch(): void {
    this.write(WATCH_ID, watchCommand(watchedCardId, watchedExtraPlace));
  }

  send(command: unknown): Promise<unknown> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      if (!this.write(id, command)) this.settle(id, { code: "closed", message: "not connected" }, undefined);
    });
  }

  end(): void {
    if (this.probeTimer !== null) window.clearTimeout(this.probeTimer);
    this.probeTimer = null;
    if (liveDial === this) liveDial = null;
    if (activeDial === this) activeDial = null;
    for (const p of this.pending.values()) p.reject(new Error("the connection closed"));
    this.pending.clear();
  }

  close(): void {
    this.end();
    this.ws.close();
  }

  private onSetup(id: number, err: ErrorBody | null): void {
    if (err === null) {
      if (id === PROBE_ID) {
        activeDial = this;
        this.rewatch();
      }
      return;
    }
    if (err.code !== "recovering") {
      this.ws.close();
      return;
    }
    this.update({ recovering: err.message });
    this.probeTimer = window.setTimeout(() => {
      this.probeTimer = null;
      this.write(PROBE_ID, { status: {} });
    }, PROBE_MS);
  }

  private settle(id: number, err: ErrorBody | null, ok: unknown): void {
    const p = this.pending.get(id);
    if (p === undefined) return;
    this.pending.delete(id);
    if (err === null) p.resolve(ok);
    else p.reject(new Error(err.message));
  }

  private write(id: number, cmd: unknown): boolean {
    if (this.ws.readyState !== WebSocket.OPEN) return false;
    this.ws.send(JSON.stringify({ id, cmd }));
    return true;
  }
}

/** Sends one command as the operator and resolves with its `ok` body; rejects with the engine's words on refusal or a drop. */
export function requestCommand(command: unknown): Promise<unknown> {
  if (liveDial === null) return Promise.reject(new Error("not connected"));
  return liveDial.send(command);
}

/** Sends one command as the operator over the live socket; rejects with the engine's words on refusal or a drop. */
/** A message's image from the bridge, fetched with the key, as an object URL its caller revokes (fac-696). */
export async function fetchImage(url: string): Promise<string> {
  const key = resolveKey();
  if (key === null) throw new Error("no bridge key");
  const res = await fetch(url, { headers: { Authorization: `Bearer ${key}` } });
  if (!res.ok) throw new Error(`image: ${res.status}`);
  return URL.createObjectURL(await res.blob());
}

export function sendCommand(command: unknown): Promise<void> {
  return requestCommand(command).then(() => undefined);
}

/** Watches the open card's Build and Spec rooms beside the screen, or the screen alone when `null`. */
export function watchCard(cardId: string | null): void {
  if (cardId === watchedCardId && watchedExtraPlace === null) return;
  watchedCardId = cardId;
  watchedExtraPlace = null;
  activeUpdate?.((s) => ({ views: keepWatched(s.views, cardId, null) }));
  activeDial?.rewatch();
}

/** Watches the selected seat or backlog card beside the current page, or drops it when `null`. */
export function watchPlace(place: string | null): void {
  if (place === watchedExtraPlace) return;
  watchedExtraPlace = place;
  activeUpdate?.((s) => ({ views: keepWatched(s.views, watchedCardId, place) }));
  activeDial?.rewatch();
}

function openSocket(url: string, onState: (state: BridgeState) => void): () => void {
  let closed = false;
  let dial: Dial | null = null;
  let timer: number | null = null;
  let state: BridgeState = INITIAL_STATE;
  const update: Update = (next) => {
    state = { ...state, ...(typeof next === "function" ? next(state) : next) };
    onState(state);
  };
  activeUpdate = update;

  const connect = () => {
    if (closed) return;
    const ws = new WebSocket(url);
    const current = new Dial(ws, update);
    dial = current;
    ws.onmessage = (event) => {
      if (!closed && dial === current) current.receive(event.data);
    };
    ws.onclose = () => {
      if (closed || dial !== current) return;
      current.end();
      dial = null;
      const link = state.screen !== null ? "reconnecting" : current.hadHello ? "connecting" : "disconnected";
      update({ link, recovering: null });
      timer = window.setTimeout(() => {
        timer = null;
        connect();
      }, RETRY_MS);
    };
  };

  update({});
  connect();

  return () => {
    closed = true;
    if (activeUpdate === update) activeUpdate = null;
    if (timer !== null) window.clearTimeout(timer);
    dial?.close();
  };
}

/** Connects to `/ws` with the bridge key, re-dialing after a drop; returns what stops it. */
export function connectBridge(onState: (state: BridgeState) => void): () => void {
  let stop: (() => void) | null = null;

  const start = (key: string) => {
    stop?.();
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${window.location.host}/ws?key=${encodeURIComponent(key)}`;
    stop = openSocket(url, onState);
  };

  const onHashChange = () => {
    const nextKey = takeAndStoreFragmentKey();
    if (nextKey !== null) start(nextKey);
  };

  window.addEventListener("hashchange", onHashChange);
  const initialKey = resolveKey();
  if (initialKey === null) {
    onState({ ...INITIAL_STATE, link: "disconnected" });
  } else {
    start(initialKey);
  }

  return () => {
    window.removeEventListener("hashchange", onHashChange);
    stop?.();
  };
}
