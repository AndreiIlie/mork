import { useState } from "react";
import { Markdown } from "./Markdown";
import { TriagePanel } from "./Triage";
import { sendCommand } from "./bridge";
import { ago, coldMinutes } from "./format";
import { ChevronIcon, Cold } from "./icons";
import { OfferButtons, RefusalLine, useHandOffers } from "./offers";
import type { BriefView, HandView, TriageView } from "./types";

export function AskLabel({ hand, nowMs }: { hand: HandView; nowMs: number }) {
  const cold = coldMinutes(hand.cold_at_ms, nowMs);
  const pill = hand.given ? "bg-gray-500" : hand.color === "red" ? "bg-red-700" : "bg-amber-700";
  return (
    <>
      <span className={`flex h-5 items-center px-2 text-label-12 font-medium text-background-100 ${pill}`}>Raised Hand</span>
      {hand.design && <span className="text-label-13 text-gray-900">design</span>}
      {hand.given && <span className="text-label-13 text-gray-900">sent to orchestrator</span>}
      {cold !== null && <Cold minutes={cold} />}
      <span className="font-mono text-label-12 tabular-nums text-gray-700">{ago(nowMs - hand.raised_ms)}</span>
    </>
  );
}

export function InlineAsk({ hand, triage, live }: { hand: HandView; triage?: TriageView; live: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const busy = pending[hand.ask] ?? null;
  const refusal = refusals[hand.ask] ?? null;
  const [answering, setAnswering] = useState<string | null>(null);
  const [answerError, setAnswerError] = useState<string | null>(null);
  // Options and the seat's pick come typed from its hand; an ask without them is plain prose.
  const choices = hand.kind === "answer" ? hand.choices : undefined;
  const options = (choices?.options ?? []).map((text, i) => ({ n: String(i + 1), text }));
  const pick = choices?.pick !== undefined ? String(choices.pick) : undefined;
  const why = choices?.reason ?? "";

  const choose = (n: string) => {
    if (!live || answering !== null || busy !== null) return;
    setAnswering(n);
    setAnswerError(null);
    sendCommand({ say: { place: hand.message_place, text: n, choice: Number(n) } })
      .catch((e: unknown) => setAnswerError(e instanceof Error ? e.message : String(e)))
      .finally(() => setAnswering(null));
  };

  if (hand.kind === "triage" && triage?.ask === hand.ask && hand.card !== null) {
    return <TriagePanel cardId={hand.card} hand={hand} triage={triage} live={live}
      pending={busy} refusal={refusal} onPress={(offer) => press(hand.ask, offer)} />;
  }
  return (
    <div className="mt-2 max-w-[86ch]">
      {hand.kind === "approve-brief" && <ApproveBody words={hand.words} brief={hand.brief} decisions={hand.decisions ?? []} />}
      {options.length > 0 && (
        <div className="my-3 flex flex-col gap-2">
          {options.map((o) => (
            <button
              key={o.n}
              type="button"
              disabled={!live || answering !== null || busy !== null}
              onClick={() => choose(o.n)}
              className={`flex min-h-8 cursor-pointer items-baseline gap-3 border px-3 py-[5px] text-left text-label-14 text-gray-1000 transition-colors hover:border-gray-900 hover:bg-gray-200 disabled:cursor-not-allowed disabled:opacity-50 ${
                o.n === pick ? "border-gray-700 bg-gray-200" : "border-gray-400 bg-background-100"
              }`}
            >
              <span className="font-mono text-label-13 tabular-nums">{o.n}</span>
              <span className="min-w-0 flex-1">
                {o.text}
                {o.n === pick && (
                  <span className="mt-0.5 block text-label-13 text-gray-900">
                    {hand.role ?? "seat"}&apos;s pick{why !== "" ? `: ${why}` : ""}
                  </span>
                )}
              </span>
            </button>
          ))}
        </div>
      )}
      <OfferButtons offers={hand.offers} pending={busy} live={live && answering === null}
        onPress={(offer) => press(hand.ask, offer)} className="mt-2 flex flex-wrap items-center gap-2" />
      <RefusalLine refusal={answerError ?? refusal} className="mt-2" />
    </div>
  );
}

const bullets = (lines: string[]) => lines.map((l) => `- ${l}`).join("\n");

// The plan's approve: its typed sketch's changes and the card's decisions; its code shape folds.
// A recap without the typed sketch shows its words as plain prose, the decisions still under them.
function ApproveBody({ words, brief, decisions }: { words: string; brief?: BriefView; decisions: string[] }) {
  const [open, setOpen] = useState(false);
  const changes = brief?.changes ?? [];
  const code = brief?.code_shape ?? [];
  const head = "mb-1 text-label-13 font-medium text-gray-1000";
  return (
    <div className="flex flex-col gap-3 text-copy-14 text-gray-1000">
      {brief === undefined && <Markdown text={keepLines(words)} className="text-gray-1000" />}
      {changes.length > 0 && (
        <section>
          <h3 className={head}>What changes</h3>
          <Markdown text={bullets(changes)} className="text-gray-1000" />
        </section>
      )}
      {decisions.length > 0 && (
        <section>
          <h3 className={head}>Decisions</h3>
          <ol className="list-decimal space-y-1 pl-5">
            {decisions.map((d, i) => (
              <li key={i}>{d}</li>
            ))}
          </ol>
        </section>
      )}
      {code.length > 0 && (
        <section>
          <button
            type="button"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
            className="flex cursor-pointer items-center gap-2 text-label-13 text-gray-900 hover:text-gray-1000"
          >
            <ChevronIcon open={open} className="text-gray-700" />
            <span>Code shape</span>
            <span className="font-mono text-label-12 tabular-nums text-gray-700">{code.length}</span>
          </button>
          {open && <Markdown text={bullets(code)} className="mt-1 pl-5 text-gray-900" />}
        </section>
      )}
      {brief?.mock !== undefined && <Markdown text={`mock: ${brief.mock}`} className="text-gray-1000" />}
    </div>
  );
}

// An ask's lines stay apart: a list ends at a plain line, and plain lines break where the seat broke them.
export function keepLines(text: string): string {
  const isItem = (l: string) => /^\s*(\d+\.|[-*])\s+/.test(l);
  const lines = text.split("\n");
  return lines
    .map((l, i) => {
      if (i === 0) return l;
      const prev = lines[i - 1];
      if (prev.trim() === "" || l.trim() === "") return `\n${l}`;
      if (isItem(prev) && !isItem(l)) return `\n\n${l}`;
      if (!isItem(prev) && !isItem(l)) return `  \n${l}`;
      return `\n${l}`;
    })
    .join("");
}
