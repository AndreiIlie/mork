import { useEffect, useState } from "react";
import { requestCommand } from "./bridge";
import type { Quality, QualityAnswer } from "./types";

export function useQuality(live: boolean) {
  const [data, setData] = useState<Quality | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!live) return;
    let active = true;
    let pending = false;
    async function load() {
      if (pending) return;
      pending = true;
      try {
        const raw = await requestCommand({ quality: {} });
        const value = (raw as QualityAnswer | null)?.quality;
        if (!value) throw new Error("Quality returned no figures.");
        if (active) {
          setData(value);
          setError(null);
        }
      } catch (e) {
        if (active) setError(e instanceof Error ? e.message : String(e));
      } finally {
        pending = false;
      }
    }
    void load();
    const timer = window.setInterval(load, 60_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [live]);
  return { data, error };
}
