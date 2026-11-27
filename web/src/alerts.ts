import { useCallback, useEffect, useRef, useState } from "react";
import type { Link } from "./bridge";
import type { HandView, Screen } from "./types";

export type FaviconTone = "red" | "amber" | null;

export interface AlertPermission {
  permission: NotificationPermission | "unsupported";
  request: () => void;
}

const BASE_TITLE = "Mork";
// Matches web/src/index.css tokens: background-100, gray-1000, amber-700, red-700.
const BG = "hsl(0,0%,4%)";
const FG = "hsl(0,0%,93%)";
const BADGE_COLOR: Record<Exclude<FaviconTone, null>, string> = {
  amber: "hsl(39,100%,57%)",
  red: "hsl(358,75%,59%)",
};

/** Open hands waiting on the operator; excludes hands already sent to an orchestrator. */
export function ownHands(hands: HandView[]): HandView[] {
  return hands.filter((h) => !h.given);
}

/** Browser tab title carrying the waiting hand count when non-zero. */
export function tabTitle(count: number): string {
  return count > 0 ? `(${count}) ${BASE_TITLE}` : BASE_TITLE;
}

/** Highest-severity favicon dot colour for the waiting hands: red outranks amber. */
export function faviconTone(hands: HandView[]): FaviconTone {
  if (hands.length === 0) return null;
  return hands.some((h) => h.color === "red") ? "red" : "amber";
}

/** Data-URI SVG favicon matching MorkMark with an optional top-right status dot. */
export function faviconHref(tone: FaviconTone): string {
  const dot =
    tone === null
      ? ""
      : `<circle cx="18.6" cy="5.4" r="5.4" fill="${BG}"/><circle cx="18.6" cy="5.4" r="4.2" fill="${BADGE_COLOR[tone]}"/>`;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">` +
    `<rect width="24" height="24" fill="${BG}"/>` +
    `<path d="M2 20V5L12 12V5L22 12V20H18.5V13.8L12 9.2V16.2L5.5 11.6V20H2Z" fill="${FG}"/>` +
    `${dot}</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/** Browser notification heading naming the brand and the hand's place. */
export function notificationTitle(hand: HandView): string {
  return `Mork · ${hand.place}`;
}

function setFavicon(href: string): void {
  let link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
  if (link === null) {
    link = document.createElement("link");
    link.rel = "icon";
    link.type = "image/svg+xml";
    document.head.appendChild(link);
  }
  if (link.href !== href) link.href = href;
}

function readPermission(): NotificationPermission | "unsupported" {
  if (typeof window === "undefined" || typeof Notification === "undefined") return "unsupported";
  return Notification.permission;
}

/** Tracks browser Notification permission and exposes a click-triggered request while unset. */
export function useNotificationPermission(): AlertPermission {
  const [permission, setPermission] = useState(readPermission);

  useEffect(() => {
    const sync = () => setPermission(readPermission());
    window.addEventListener("focus", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      window.removeEventListener("focus", sync);
      document.removeEventListener("visibilitychange", sync);
    };
  }, []);

  const request = useCallback(() => {
    if (typeof Notification === "undefined") return;
    void Notification.requestPermission()
      .then((next) => setPermission(next))
      .catch(() => setPermission(readPermission()));
  }, []);

  return { permission, request };
}

/** Drives document.title, the SVG favicon badge, and unfocused browser notifications for raised hands. */
export function useHandAlerts(screen: Screen | null, link: Link, onOpenHand: (hand: HandView) => void): void {
  const onOpenRef = useRef(onOpenHand);
  onOpenRef.current = onOpenHand;
  const primedRef = useRef(false);
  const seenRef = useRef<Set<string>>(new Set());
  const activeRef = useRef<Map<string, Notification>>(new Map());

  useEffect(() => {
    const hands = screen !== null ? ownHands(screen.hands) : [];
    document.title = tabTitle(hands.length);
    setFavicon(faviconHref(faviconTone(hands)));
  }, [screen]);

  useEffect(() => {
    const active = activeRef.current;
    return () => {
      document.title = BASE_TITLE;
      setFavicon(faviconHref(null));
      for (const note of active.values()) note.close();
      active.clear();
    };
  }, []);

  useEffect(() => {
    if (link !== "live" || screen === null) {
      // Re-prime on the next live screen so reconnects never replay existing open hands.
      primedRef.current = false;
      return;
    }

    const hands = ownHands(screen.hands);
    const current = new Set(hands.map((h) => h.ask));
    for (const [ask, note] of activeRef.current) {
      if (!current.has(ask)) {
        note.close();
        activeRef.current.delete(ask);
      }
    }

    if (!primedRef.current) {
      primedRef.current = true;
      seenRef.current = current;
      return;
    }

    const added = hands.filter((h) => !seenRef.current.has(h.ask));
    seenRef.current = current;
    if (added.length === 0) return;
    if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
    if (!document.hidden && document.hasFocus()) return;

    for (const hand of added) {
      try {
        const icon = faviconHref(hand.color === "red" ? "red" : "amber");
        const note = new Notification(notificationTitle(hand), {
          body: hand.words,
          tag: hand.ask,
          icon,
        });
        activeRef.current.set(hand.ask, note);
        note.onclose = () => {
          if (activeRef.current.get(hand.ask) === note) activeRef.current.delete(hand.ask);
        };
        note.onclick = () => {
          window.focus();
          onOpenRef.current(hand);
          note.close();
        };
      } catch {
        // Ignore browser environments that disallow the Notification constructor.
      }
    }
  }, [screen, link]);
}
