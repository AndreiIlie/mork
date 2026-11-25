import { Fragment, useState } from "react";
import { HandCard } from "./HandCard";
import { Markdown } from "./Markdown";
import { TriagePanel } from "./Triage";
import { sendCommand } from "./bridge";
import { ago, coldMinutes, plural, wherePlace } from "./format";
import { ChevronIcon, Cold } from "./icons";
import { OfferButtons, RefusalLine, useHandOffers } from "./offers";
import type { BriefView, HandView, Offer, TriageView } from "./types";

function handWhere(hand: HandView, ownRoom: string): string {
  const below = wherePlace(hand);
  if (below === "spec") return "Spec";
  if (below === "") return ownRoom;
  return below;
}

function AskPanel({
  hand,
  ownRoom,
  nowMs,
  live,
  pending,
  refusal,
  onPress,
}: {
  hand: HandView;
  ownRoom: string;
  nowMs: number;
  live: boolean;
  pending: string | null;
  refusal: string | null;
  onPress: (offer: Offer) => void;
}) {
  const [answering, setAnswering] = useState<string | null>(null);
  const [answerError, setAnswerError] = useState<string | null>(null);
  const cold = coldMinutes(hand.cold_at_ms, nowMs);
  const where = handWhere(hand, ownRoom);
  const heading = hand.role !== null ? `${hand.role} asks in ${where}` : `${hand.kind} in ${where}`;
  // Options and the seat's pick come typed from its hand (fac-696); an ask without them is plain prose.
  const choices = hand.kind === "answer" ? hand.choices : undefined;
  const options = (choices?.options ?? []).map((text, i) => ({ n: String(i + 1), text }));
  const pick = choices?.pick !== undefined ? String(choices.pick) : undefined;
  const why = choices?.reason ?? "";

  const choose = (n: string) => {
    if (!live || answering !== null || pending !== null) return;
    setAnswering(n);
    setAnswerError(null);
    sendCommand({ say: { place: hand.place, text: n, choice: Number(n) } })
      .catch((e: unknown) => setAnswerError(e instanceof Error ? e.message : String(e)))
      .finally(() => setAnswering(null));
  };

  return (
    <HandCard
      color={hand.color}
      head={
        <>
          <span className="text-label-13 text-gray-900">{heading}</span>
          {hand.design && <span className="text-label-13 text-gray-900">design</span>}
          {hand.given && <span className="text-label-13 text-gray-900">sent to orchestrator</span>}
          {cold !== null && <Cold minutes={cold} />}
          <span className="font-mono text-label-12 tabular-nums text-gray-700">{ago(nowMs - hand.raised_ms)}</span>
        </>
      }
      actions={
        hand.offers.length > 0 ? (
          <OfferButtons
            offers={hand.offers}
            live={live && answering === null}
            pending={pending}
            onPress={onPress}
            className="flex flex-wrap items-center gap-2"
          />
        ) : undefined
      }
    >
      <div className="px-4 py-3">
        <div className="max-w-[86ch]">
          {hand.kind === "approve-brief" ? (
            <ApproveBody words={hand.words} brief={hand.brief} decisions={hand.decisions ?? []} />
          ) : (
            <Markdown text={keepLines(choices?.ask ?? hand.words)} className="text-gray-1000" />
          )}
          {options.length > 0 && (
            <div className="my-3 flex flex-col gap-2">
              {options.map((o) => (
                <button
                  key={o.n}
                  type="button"
                  disabled={!live || answering !== null || pending !== null}
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
        </div>
      </div>
      <RefusalLine refusal={answerError ?? refusal} className="px-4 pb-3" />
    </HandCard>
  );
}

const bullets = (lines: string[]) => lines.map((l) => `- ${l}`).join("\n");

// The plan's approve: its typed sketch's changes and the card's decisions; its code shape folds (fac-696).
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

function FoldedHand({
  hand,
  triage,
  ownRoom,
  nowMs,
  onOpen,
}: {
  hand: HandView;
  triage: TriageView | undefined;
  ownRoom: string;
  nowMs: number;
  onOpen: () => void;
}) {
  const where = handWhere(hand, ownRoom);
  const isTriage = hand.kind === "triage" && triage !== undefined && triage.ask === hand.ask;
  const heading = isTriage
    ? `Review round ${triage.round} ${triage.stalemate ? "stalemate" : "failed"} · ${triage.findings.length} ${plural(triage.findings.length, "finding", "findings")} to triage`
    : hand.role !== null
      ? `${hand.role} asks in ${where}`
      : `${hand.kind} in ${where}`;
  const pill = hand.color === "red" ? "bg-red-700" : hand.color === "yellow" ? "bg-amber-700" : "bg-gray-500";
  return (
    <div className="px-gutter pt-3">
      <button
        type="button"
        onClick={onOpen}
        aria-expanded={false}
        className="flex w-full cursor-pointer items-center gap-3 border border-gray-500 bg-gray-100 px-4 py-2 text-left transition-colors hover:bg-gray-200"
      >
        <span className={`flex h-5 shrink-0 items-center px-2 text-label-12 font-medium text-background-100 ${pill}`}>
          Raised Hand
        </span>
        <span className="shrink-0 text-label-13 text-gray-900">{heading}</span>
        {!isTriage && <span className="min-w-0 truncate text-label-13 text-gray-1000">{hand.words.split("\n", 1)[0]}</span>}
        <span className="ml-auto shrink-0 font-mono text-label-12 tabular-nums text-gray-700">
          {ago(nowMs - hand.raised_ms)}
        </span>
        <span className="shrink-0 text-label-13 text-gray-900">Open</span>
      </button>
    </div>
  );
}

interface HandsProps {
  cardId: string;
  hands: HandView[];
  triage: TriageView | undefined;
  ownRoom: string;
  openAsk: string | undefined;
  onOpenHand: (hand: HandView) => void;
  nowMs: number;
  live: boolean;
}

/** The card's Raised Hands dock: one hand shown whole, any others folded to one line. */
export function RoomHands({ cardId, hands, triage, ownRoom, openAsk, onOpenHand, nowMs, live }: HandsProps) {
  const { pending, refusals, press } = useHandOffers();
  if (hands.length === 0) return null;

  const cards = hands.map((hand) => {
    const folded = openAsk !== undefined && hand.ask !== openAsk;
    if (hand.kind === "triage" && triage !== undefined && triage.ask === hand.ask) {
      return (
        <Fragment key={hand.ask}>
          {folded && <FoldedHand hand={hand} triage={triage} ownRoom={ownRoom} nowMs={nowMs} onOpen={() => onOpenHand(hand)} />}
          <div className={folded ? "hidden" : undefined}>
            <TriagePanel
              cardId={cardId}
              hand={hand}
              triage={triage}
              nowMs={nowMs}
              live={live}
              pending={pending[hand.ask] ?? null}
              refusal={refusals[hand.ask] ?? null}
              onPress={(o) => press(hand.ask, o)}
            />
          </div>
        </Fragment>
      );
    }
    if (folded) {
      return <FoldedHand key={hand.ask} hand={hand} triage={triage} ownRoom={ownRoom} nowMs={nowMs} onOpen={() => onOpenHand(hand)} />;
    }
    return (
      <AskPanel
        key={hand.ask}
        hand={hand}
        ownRoom={ownRoom}
        nowMs={nowMs}
        live={live}
        pending={pending[hand.ask] ?? null}
        refusal={refusals[hand.ask] ?? null}
        onPress={(o) => press(hand.ask, o)}
      />
    );
  });
  return <div>{cards}</div>;
}
