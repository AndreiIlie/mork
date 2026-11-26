import { useEffect, useState } from "react";
import { watchPlace } from "./bridge";
import { RefusalLine, useHandOffers } from "./offers";
import type { BoardRow, PlaceView, StartDefaults } from "./types";
import { Button } from "./ui/Button";
import { TextInput } from "./ui/TextInput";
import { Segmented } from "./ui/Segmented";

const INPUT = "w-full border border-gray-400 bg-background-100 px-3 text-label-13 text-gray-1000 placeholder:text-gray-700";
const FIELD = "text-label-12 text-gray-700";

export function NewCard({ projects, project, live, onDone }: { projects: string[]; project: string; live: boolean; onDone: () => void }) {
  const { press, pending, refusals } = useHandOffers();
  const [where, setWhere] = useState(project);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  return (
    <section className="mb-gutter border border-gray-400">
      <div className="flex items-center gap-3 border-b border-gray-400 px-edge py-3">
        <h2 className="text-heading-14 text-gray-1000">New card</h2>
        <span className="ml-auto">
          <Segmented options={projects} value={where} onChange={setWhere} />
        </span>
      </div>
      <div className="flex flex-col gap-3 px-edge py-4">
        <label className="flex flex-col gap-1.5">
          <span className={FIELD}>Title</span>
          <TextInput value={title} onChange={(e) => setTitle(e.target.value)} placeholder="What should change, in one line" />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className={FIELD}>Description</span>
          <textarea
            className={`${INPUT} h-[96px] resize-none py-2`}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="The issue, in your own words"
          />
        </label>
        <div className="flex items-center gap-2">
          <Button tier="primary" disabled={!live || !projects.includes(where) || !title.trim() || !!pending.create} onClick={() => press("create", { label: "Create", command: { "card.create": { project: where, title, description } } }, onDone)}>
            Create in {where}
          </Button>
          <Button tier="ghost" disabled={!!pending.create} onClick={onDone}>
            Cancel
          </Button>
          <span className="ml-auto text-label-12 text-gray-700">The card lands in {where}&apos;s backlog, not started.</span>
        </div>
        <RefusalLine refusal={refusals.create ?? null} className="" />
      </div>
    </section>
  );
}

const LEVELS = ["Off", "Delegated", "Self-sustained"] as const;

// A start names Delegated, Self-sustained or no level: with none the card keeps its last run's level, else its
// project's auto, so Off can't be asked at start once either delegates it.
function StartForm({ card, defaults, live, onDone }: { card: BoardRow; defaults: StartDefaults; live: boolean; onDone: () => void }) {
  const [mode, setMode] = useState<"Full" | "Report">(defaults.modes.full ? "Full" : "Report");
  const [visual, setVisual] = useState(defaults.visual);
  const [noPlanner, setNoPlanner] = useState(false);
  const [level, setLevel] = useState<number>(defaults.level);
  const full = mode === "Full";
  const { pending, refusals, press } = useHandOffers();
  const start = () => {
    const delegation = level === defaults.level ? {} : level === 2 ? { "self-sustained": true } : { delegated: true };
    const options = full ? { visual: visual && !defaults.visual || undefined, "no-planner": noPlanner || undefined, carries: defaults.carries } : {};
    press("start", { label: "Start", command: { start: { card: card.id, mode: full ? "full" : "report", ...options, ...delegation } } }, onDone);
  };
  const kept = [
    defaults.from === "run" && defaults.level > 0 ? LEVELS[defaults.level] : null,
    defaults.visual ? "Visual" : null,
    defaults.carries.length > 0 ? `carrying ${defaults.carries.join(" ")}` : null,
  ].filter(Boolean);
  const inherited = [
    kept.length > 0 ? `From its last run: ${kept.join(", ")}` : null,
    defaults.from === "project" && defaults.level > 0 ? "Delegated by the project's Auto delegation" : null,
  ].filter(Boolean);
  const modeOff = (m: "Full" | "Report") => !(m === "Full" ? defaults.modes.full : defaults.modes.report);
  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-3 bg-background-200 py-3 pl-[calc(var(--spacing-edge)+24px)] pr-edge">
      <span className="flex items-center gap-2">
        <span className={FIELD}>Mode</span>
        <Segmented height={28}
          options={["Full", "Report"] as const}
          value={mode}
          onChange={setMode}
          disabled={modeOff}
        />
      </span>
      <span className="flex items-center gap-2">
        <Button tier="secondary" size="control" pressed={full && visual} disabled={!full || defaults.visual} onClick={() => setVisual((v) => !v)} title={defaults.visual ? "Its last run was visual: this one is too" : undefined}>
          Visual
        </Button>
        <Button tier="secondary" size="control" pressed={full && noPlanner} disabled={!full} onClick={() => setNoPlanner((v) => !v)}>
          No planner
        </Button>
      </span>
      <span className="flex items-center gap-2">
        <span className={FIELD}>Delegation</span>
        <Segmented height={28}
          options={LEVELS}
          value={LEVELS[level]}
          onChange={(name) => setLevel(LEVELS.indexOf(name))}
          disabled={(name) => name === "Off" && defaults.level > 0}
        />
      </span>
      {inherited.length > 0 && <span className="text-label-12 text-gray-700">{inherited.join(" · ")}</span>}
      <span className="ml-auto flex items-center gap-2">
        <Button size="control" tier="ghost" disabled={!!pending.start} onClick={onDone}>
          Cancel
        </Button>
        <Button size="control" tier="primary" disabled={!live || !!pending.start || modeOff(mode)} onClick={start}>
          {card.ran ? "Start again" : "Start"} {card.id}
        </Button>
      </span>
      <RefusalLine refusal={refusals.start ?? null} className="w-full" />
    </div>
  );
}

export function StartStrip({ card, view, live, onDone }: { card: BoardRow; view?: PlaceView; live: boolean; onDone: () => void }) {
  useEffect(() => {
    watchPlace(card.id);
    return () => watchPlace(null);
  }, [card.id]);
  if (!view?.start) return <div className="flex items-center gap-3 bg-background-200 px-edge py-3 text-label-13">
    <span>{view ? `${card.id} can't start now.` : "Loading…"}</span><Button tier="ghost" size="control" onClick={onDone}>Cancel</Button>
  </div>;
  return <StartForm key={JSON.stringify(view.start)} card={card} defaults={view.start} live={live} onDone={onDone} />;
}
