import { useCallback, useEffect, useRef, useState } from "react";
import { requestCommand, sendCommand } from "./bridge";
import type { ConfigChange, ConfigSection, ConfigValue, ConfigView } from "./types";

export function useConfig(live: boolean) {
  const [view, setView] = useState<ConfigView | null>(null);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const refresh = useCallback(async () => {
    const id = ++generation.current;
    try {
      const answer = await requestCommand({ config: {} }) as { config: ConfigView };
      if (id === generation.current) {
        setView(answer.config);
        setError("");
      }
      return answer.config;
    } catch (e) {
      if (id === generation.current) setError(e instanceof Error ? e.message : String(e));
      throw e;
    }
  }, []);
  useEffect(() => {
    if (!live) return;
    const load = () => { void refresh().catch(() => {}); };
    load();
    window.addEventListener("focus", load);
    return () => { ++generation.current; window.removeEventListener("focus", load); };
  }, [live, refresh]);
  return { view, error, refresh };
}

export type RefreshConfig = () => Promise<ConfigView>;

function same(a: ConfigValue, b: ConfigValue) {
  const ordered = (_: string, v: unknown) => v && typeof v === "object" && !Array.isArray(v)
    ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b))) : v;
  return JSON.stringify(a, ordered) === JSON.stringify(b, ordered);
}

export function useConfigSection(section: ConfigSection, refresh: RefreshConfig) {
  const [draft, setDraft] = useState<ConfigChange["fields"]>({});
  const [refusal, setRefusal] = useState("");
  const [busy, setBusy] = useState(false);
  const fields = new Map(section.fields.map(f => [f.key, f]));
  function value(key: string) {
    const current = fields.get(key)?.value;
    return current ? draft[key]?.value ?? current : null;
  }
  function edit(key: string, value: ConfigValue) {
    const current = fields.get(key)?.value;
    if (!current) return;
    setDraft(previous => {
      const next = { ...previous };
      const read = previous[key]?.read ?? current;
      if (same(current, value)) delete next[key];
      else next[key] = { read, value };
      return next;
    });
    setRefusal("");
  }
  async function save() {
    if (busy) return;
    setBusy(true);
    const submitted = { ...draft };
    for (const [key, edit] of Object.entries(draft)) {
      if (!key.endsWith(".mode") || !("text" in edit.value) || edit.value.text !== "auto") continue;
      for (const threshold of ["landings", "edits", "notes", "days"]) {
        const name = key.slice(0, -4) + threshold;
        submitted[name] ??= { read: { text: "" }, value: { text: "" } };
      }
    }
    try {
      const change: ConfigChange = { section: section.section, project: section.project, fields: submitted };
      await sendCommand({ "config.change": { change } });
      setDraft({});
      setRefusal("");
      await refresh();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setRefusal(message);
      const latest = await refresh().catch(() => null);
      const fresh = latest?.sections.find(s => s.section === section.section && s.project === section.project);
      if (fresh && message.includes("in another window since this page read it")) {
        const next = { ...submitted };
        const differences: string[] = [];
        for (const field of fresh.fields) {
          const pending = next[field.key];
          if (!pending || !field.value || same(field.value, pending.read)) continue;
          next[field.key] = { ...pending, read: field.value };
          const shown = "text" in field.value ? field.value.text : JSON.stringify(field.value);
          const label = field.key === "slots" ? "Test slots" : field.key;
          differences.push(label + " changed to " + shown + " in another window since this page read it.");
        }
        setDraft(next);
        if (differences.length) setRefusal(differences.join(" ") + " Your edit is kept: check it and save again.");
      }
    } finally { setBusy(false); }
  }
  function discard() { setDraft({}); setRefusal(""); }
  return { section, fields, value, edit, dirty: Object.keys(draft).length > 0, busy, refusal, save, discard };
}

export type Editor = ReturnType<typeof useConfigSection>;
