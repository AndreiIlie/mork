import type { MouseEvent, ReactNode } from "react";

export type Route = { page: "board" } | { page: "usage" } | { page: "card"; id: string; hand: string | null };

export function parseRoute(pathname: string, search = ""): Route {
  if (/^\/usage\/?$/.test(pathname)) return { page: "usage" };
  const match = /^\/cards\/([^/]+)\/?$/.exec(pathname);
  if (match === null) return { page: "board" };
  try {
    const rawHand = new URLSearchParams(search).get("hand");
    const hand = rawHand !== null && rawHand !== "" ? rawHand : null;
    return { page: "card", id: decodeURIComponent(match[1]), hand };
  } catch {
    return { page: "board" };
  }
}

export function cardPath(id: string, hand?: string | null): string {
  const base = `/cards/${encodeURIComponent(id)}`;
  return hand !== undefined && hand !== null && hand !== "" ? `${base}?hand=${encodeURIComponent(hand)}` : base;
}

/** Moves to `path` in this tab's history; App re-reads the route on `popstate`. */
export function navigate(path: string): void {
  if (path === window.location.pathname + window.location.search) return;
  window.history.pushState(null, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

/** Replaces the current history entry with `path` (used to redirect folded review cards or clear `?hand=`). */
export function replaceRoute(path: string): void {
  if (path === window.location.pathname + window.location.search) return;
  window.history.replaceState(null, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

/** A real link: a plain click routes in place, a modified or middle click is the browser's. */
export function PageLink({ to, className, children }: { to: string; className?: string; children: ReactNode }) {
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    navigate(to);
  };
  return (
    <a href={to} className={className} onClick={onClick}>
      {children}
    </a>
  );
}
