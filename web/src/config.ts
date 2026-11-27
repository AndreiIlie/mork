import { useCallback, useEffect, useRef, useState } from "react";
import { requestCommand, sendCommand } from "./bridge";
import type { ConfigChange, ConfigField, ConfigSection, ConfigValue, ConfigView } from "./types";

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

type Draft = Record<string, { read: ConfigValue; value: ConfigValue; label: string; baseline: ConfigSection }>;

function submission(draft: Draft) {
  const fields: ConfigChange["fields"] = {};
  const labels: Record<string, string> = {};
  for (const [key, { read, value, label }] of Object.entries(draft)) {
    fields[key] = { read, value };
    labels[key] = label;
  }
  for (const [key, { value, label, baseline }] of Object.entries(draft)) {
    if (!key.endsWith(".mode") || !("text" in value) || value.text !== "auto") continue;
    for (const threshold of ["landings", "edits", "notes", "days"]) {
      const name = key.slice(0, -4) + threshold;
      const read = fields[name]?.read ?? baseline.fields.find(f => f.key === name)?.value;
      // Compare the thresholds Auto will remove, including ones added by another window.
      if (read) {
        fields[name] = { read, value: read };
        labels[name] = label + " " + threshold;
      }
    }
  }
  return { fields, labels };
}

function shown(field: ConfigField, value: ConfigValue) {
  if ("seat" in value) {
    const seat = value.seat;
    const model = field.offers.find(m => m.cli === seat.cli && m.model === seat.model);
    return (seat.configured ? "" : "default ") + (model?.label ?? seat.model) + " · " + seat.effort;
  }
  if ("picks" in value) return value.picks.map(m => m.label + " (" + m.efforts.join(", ") + ")").join(", ") || "none";
  if ("entries" in value) return value.entries.join(", ") || "none";
  if (field.key.endsWith(".mode") || field.key === "colleague_review") {
    return value.text.charAt(0).toUpperCase() + value.text.slice(1);
  }
  return value.text || (field.key.startsWith("passes.") ? "off" : "empty");
}

export function useConfigSection(section: ConfigSection, refresh: RefreshConfig) {
  const [draft, setDraft] = useState<Draft>({});
  const [refusal, setRefusal] = useState("");
  const [busy, setBusy] = useState(false);
  const fields = new Map(section.fields.map(f => [f.key, f]));
  function value(key: string) {
    const current = fields.get(key)?.value;
    return current ? draft[key]?.value ?? current : null;
  }
  function edit(key: string, value: ConfigValue, label: string) {
    const current = fields.get(key)?.value;
    if (!current) return;
    setDraft(previous => {
      const next = { ...previous };
      const read = previous[key]?.read ?? current;
      if (same(current, value)) delete next[key];
      else next[key] = { read, value, label, baseline: previous[key]?.baseline ?? section };
      return next;
    });
    setRefusal("");
  }
  async function save() {
    if (busy) return;
    setBusy(true);
    const submitted = submission(draft);
    try {
      const change: ConfigChange = { section: section.section, project: section.project, fields: submitted.fields };
      await sendCommand({ "config.change": { change } });
      setDraft({});
      setRefusal("");
      await refresh().catch(() => null);
    } catch (e) {
      const latest = await refresh().catch(() => null);
      const fresh = latest?.sections.find(s => s.section === section.section && s.project === section.project);
      const differences: string[] = [];
      if (fresh && !latest?.stale) {
        const next = { ...draft };
        for (const field of fresh.fields) {
          const pending = submitted.fields[field.key];
          if (!pending || !field.value) continue;
          if (!same(field.value, pending.read)) {
            differences.push(submitted.labels[field.key] + " changed to " + shown(field, field.value) + " in another window since this page read it.");
          }
          if (draft[field.key]) next[field.key] = { ...draft[field.key], read: field.value, baseline: fresh };
        }
        if (differences.length) setDraft(next);
      }
      setRefusal(differences.length ? differences.join(" ") + " Your edit is kept: check it and save again." : e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  }
  function discard() { setDraft({}); setRefusal(""); }
  return { section, fields, value, edit, dirty: Object.keys(draft).length > 0, busy, refusal, save, discard };
}

export type Editor = ReturnType<typeof useConfigSection>;
