import type { MouseEvent, ReactNode } from "react";

export type SettingsTab = "general" | "usage" | "quality" | "updates";
export type Route =
  | { page: "console" }
  | { page: "project"; project: string }
  | { page: "settings"; tab: SettingsTab }
  | { page: "card"; id: string; hand: string | null };

export function parseRoute(pathname: string, search = ""): Route {
  const path = pathname.replace(/\/$/, "");
  if (path === "/usage" || path === SETTINGS_PATH.usage) return { page: "settings", tab: "usage" };
  if (path === SETTINGS_PATH.general) return { page: "settings", tab: "general" };
  if (path === SETTINGS_PATH.quality) return { page: "settings", tab: "quality" };
  if (path === SETTINGS_PATH.updates) return { page: "settings", tab: "updates" };
  const match = /^\/(cards|projects)\/([^/]+)\/?$/.exec(pathname);
  if (!match) return { page: "console" };
  try {
    const id = decodeURIComponent(match[2]);
    if (match[1] === "projects") return { page: "project", project: id };
    const rawHand = new URLSearchParams(search).get("hand");
    const hand = rawHand !== null && rawHand !== "" ? rawHand : null;
    return { page: "card", id, hand };
  } catch {
    return { page: "console" };
  }
}

export function projectPath(project: string): string {
  return `/projects/${encodeURIComponent(project)}`;
}

export const SETTINGS_PATH: Record<SettingsTab, string> = {
  general: "/settings",
  usage: "/settings/usage",
  quality: "/settings/quality",
  updates: "/settings/updates",
};

export function cardPath(id: string, hand?: string | null): string {
  const base = `/cards/${encodeURIComponent(id)}`;
  return hand !== undefined && hand !== null && hand !== "" ? `${base}?hand=${encodeURIComponent(hand)}` : base;
}

/** Moves to `path` in this tab's history; App re-reads the route on `popstate`. */
export function navigate(path: string): void {
  if (path === window.location.pathname + window.location.search) {
    const hand = new URLSearchParams(window.location.search).get("hand");
    if (hand) document.getElementById(hand)?.scrollIntoView({ block: "center" });
    return;
  }
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
