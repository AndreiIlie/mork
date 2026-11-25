import { useEffect, useState } from "react";
import { requestCommand } from "./bridge";
import type { RecentBurn, UsageAnswer } from "./types";

const REFRESH_MS = 60_000;

function isRecentBurn(value: unknown): value is RecentBurn {
  if (typeof value !== "object" || value === null) return false;
  const b = value as Partial<RecentBurn>;
  return typeof b.total_cents === "number" && typeof b.ended_cards === "number" && Array.isArray(b.roles);
}

/** Reads the engine's rolling 7-day burn summary on open and every 60s while the bridge link is live. */
export function useRecentBurn(live: boolean): { burn: RecentBurn | null; error: string | null } {
  const [burn, setBurn] = useState<RecentBurn | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!live) return;
    let cancelled = false;
    const load = () => {
      requestCommand({ usage: {} })
        .then((raw) => {
          if (cancelled) return;
          const next = (raw as UsageAnswer | null)?.usage?.recent_burn;
          if (isRecentBurn(next)) {
            setBurn(next);
            setError(null);
          } else {
            setError("usage returned no burn summary");
          }
        })
        .catch((e: unknown) => {
          if (cancelled) return;
          setError(e instanceof Error ? e.message : String(e));
        });
    };
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [live]);

  return { burn, error };
}
