import { useState } from "react";
import { useHandOffers } from "./offers";

export type ReloadFrom = "rail" | "updates";

export function useReload(reloading: boolean) {
  const { pending, refusals, press } = useHandOffers();
  const [from, setFrom] = useState<ReloadFrom | null>(null);
  return {
    busy: pending.reload !== undefined || reloading,
    refusal: (where: ReloadFrom) => (from === where ? refusals.reload ?? null : null),
    press: (where: ReloadFrom) => {
      setFrom(where);
      press("reload", { label: "Reload", command: { reload: {} } });
    },
  };
}

export type Reload = ReturnType<typeof useReload>;
