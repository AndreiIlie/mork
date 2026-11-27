import { useState } from "react";
import { useHandOffers, RefusalLine } from "./offers";
import type { CardHeader, Color, Command } from "./types";
import { Button } from "./ui/Button";
import { Dot } from "./ui/Dot";
import { Menu } from "./ui/Menu";
import { Segmented } from "./ui/Segmented";
import { TextInput } from "./ui/TextInput";
import { TextLink } from "./ui/TextLink";

const LEVELS = ["Off", "Delegated", "Self-sustained"] as const;

export function CardControls({ card, status, live }: { card: CardHeader; status: Color; live: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const [strip, setStrip] = useState<"brief" | "close" | null>(null);
  const [words, setWords] = useState("");
  const disabled = !live || pending[card.id] !== undefined;
  const send = (label: string, command?: Command, done?: () => void) => {
    if (command) press(card.id, { label, command }, done);
  };
  const has = (name: string) => !disabled && card.about.actions?.includes(name);
  const control = (name: string) => card.about.switches?.find((s) => s.name === name);
  const active = control("active");
  const visual = control("visual");
  const delegation = control("delegation");
  const currentLevel = LEVELS[delegation?.position ?? 0];
  const levelCommand = (level: (typeof LEVELS)[number]) => delegation?.positions[LEVELS.indexOf(level)]?.command;
  const preview = card.about.preview;
  const pause = active?.positions[active.position === 1 ? 0 : 1]?.command;
  const flipVisual = visual?.positions[visual.position === 1 ? 0 : 1]?.command;
  const brief = card.stage === "starting" || card.stage === "brief";
  const stage = card.pipeline.find((p) => p.at === "current");
  const stageLabel = stage
    ? [stage.label, stage.count].filter(Boolean).join(" ")
    : card.stage.charAt(0).toUpperCase() + card.stage.slice(1);
  const stageWords = [stageLabel, stage?.sub, card.paused && "paused"].filter(Boolean).join(" · ");
  const open = (name: "brief" | "close") => {
    setWords("");
    setStrip(name);
  };
  const close = strip === "close";
  const submitStrip = () => {
    if (!strip || !has(strip) || (close && !words.trim())) return;
    const command = close ? { close: { card: card.id, why: words } } : { brief: { card: card.id, words } };
    send(close ? "Close card" : "Back to brief", command, () => setStrip(null));
  };
  return (
    <>
      <header className="shrink-0 border-b border-gray-400 px-gutter py-4">
        <div className="flex min-h-7 flex-wrap items-center gap-2 text-label-13 text-gray-900">
          <TextLink to="/">Board</TextLink>
          <span>/</span>
          <span>{card.about.project}</span>
          <span>/</span>
          <span className="font-mono text-gray-1000">{card.id}</span>
          <span className="ml-3 flex items-center gap-2 font-mono text-label-12 text-gray-1000">
            <Dot color={status} pulse={status === "green"} />
            {stageWords}
          </span>
          <div className="ml-auto flex flex-wrap items-center gap-2">
            <span className="text-label-12 text-gray-700">Delegation</span>
            <Segmented
              options={LEVELS}
              value={currentLevel}
              disabled={(level) => disabled || !delegation || (level !== currentLevel && !levelCommand(level))}
              onChange={(level) => {
                const command = levelCommand(level);
                if (command) send("Delegation", command);
              }}
            />
            <Button tier="secondary" disabled={disabled || !brief || !flipVisual} pressed={visual?.position === 1} onClick={() => send("Visual", flipVisual)}>Visual</Button>
            {preview?.on && <a href={preview.url} target="_blank" rel="noreferrer" className="font-mono text-label-12 text-gray-900 underline">{preview.url.replace(/^http:\/\//, "").replace(/\/$/, "")}</a>}
            <Button tier="secondary" pressed={preview?.on === true} disabled={disabled || !preview?.enabled} onClick={() => send("Preview", { preview: { card: card.id, action: preview?.on ? "stop" : "start" } })}>Preview</Button>
            <Button tier="secondary" pressed={card.paused} disabled={disabled || !pause} onClick={() => send(card.paused ? "Resume" : "Pause", pause)}>{card.paused ? "Resume" : "Pause"}</Button>
            <Menu items={[
              { label: "Back to brief…", disabled: !has("brief"), pick: () => open("brief") },
              { label: "Reopen mock", disabled: !has("mock"), pick: () => send("Reopen mock", { mock: { card: card.id } }) },
              { label: "Close card…", disabled: !has("close"), pick: () => open("close") }
            ]} />
          </div>
        </div>
        <RefusalLine refusal={refusals[card.id] ?? null} />
        <h1 className="mt-1.5 truncate text-heading-24 text-gray-1000">{card.title}</h1>
      </header>
      {strip && <form className="flex shrink-0 items-center gap-3 border-b border-gray-400 bg-background-200 px-gutter py-3" onSubmit={(e) => {
        e.preventDefault();
        submitStrip();
      }}>
        <label htmlFor="card-action-words" className="shrink-0 text-label-13 text-gray-1000">{close ? "Close this card" : "Send back to brief"}</label>
        <TextInput id="card-action-words" autoFocus value={words} onChange={(e) => setWords(e.target.value)} placeholder={close ? "Why it is closed" : "What the new plan should change (optional)"} />
        <Button type="submit" tier="primary" disabled={!has(strip) || (close && !words.trim())}>{close ? "Close card" : "Back to brief"}</Button>
        <Button tier="ghost" onClick={() => setStrip(null)}>Cancel</Button>
      </form>}
    </>
  );
}
