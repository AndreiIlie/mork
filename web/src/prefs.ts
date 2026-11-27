import { useEffect, useState } from "react";

const RESOURCE_TRACKER = "mork.web.resourceTracker";

function readTracker(): boolean {
  try {
    return localStorage.getItem(RESOURCE_TRACKER) !== "off";
  } catch {
    return true;
  }
}

export function useResourceTracker(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(readTracker);
  useEffect(() => {
    const sync = (event: StorageEvent) => {
      if (event.key === RESOURCE_TRACKER || event.key === null) setOn(readTracker());
    };
    window.addEventListener("storage", sync);
    return () => window.removeEventListener("storage", sync);
  }, []);
  return [on, (enabled) => {
    setOn(enabled);
    try {
      localStorage.setItem(RESOURCE_TRACKER, enabled ? "on" : "off");
    } catch {
      // The switch still applies in this tab when the browser disables storage.
    }
  }];
}
