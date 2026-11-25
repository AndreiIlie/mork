import { useState, useSyncExternalStore } from "react";
import { HandCard } from "./HandCard";
import { Markdown } from "./Markdown";
import { sendCommand } from "./bridge";
import { ago, coldMinutes, plural } from "./format";
import { ChevronIcon, Cold } from "./icons";
import { OfferButtons, RefusalLine } from "./offers";
import type { HandView, Offer, TriageView } from "./types";
import { Button } from "./ui/Button";

const CHOICES = ["Fix", "Decline", "Foreman decides"] as const;
type Choice = (typeof CHOICES)[number];

interface TriageEntry {
  choices: Record<number, Choice>;
  sending: boolean;
  sendError: string | null;
}

const EMPTY_ENTRY: TriageEntry = { choices: {}, sending: false, sendError: null };
// Choices, sending state, and send error survive folding and leaving the card's page (fac-698).
const triageStore = new Map<string, TriageEntry>();
const listeners = new Set<() => void>();

function notify() {
  for (const fn of listeners) fn();
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

function readEntry(ask: string): TriageEntry {
  return triageStore.get(ask) ?? EMPTY_ENTRY;
}

function updateEntry(ask: string, patch: Partial<TriageEntry>) {
  const prev = triageStore.get(ask) ?? EMPTY_ENTRY;
  triageStore.set(ask, { ...prev, ...patch });
  notify();
}

/** Clears stored triage state for asks that are no longer open (fac-698). */
export function pruneTriageState(openAsks: ReadonlySet<string>) {
  let changed = false;
  for (const [ask, entry] of triageStore) {
    if (!openAsks.has(ask) && !entry.sending) {
      triageStore.delete(ask);
      changed = true;
    }
  }
  if (changed) notify();
}

function ChoiceSwitch({
  value,
  disabled,
  onChange,
}: {
  value: Choice | undefined;
  disabled: boolean;
  onChange: (c: Choice) => void;
}) {
  return (
    <div className="ml-auto flex shrink-0 flex-wrap items-center gap-1">
      {CHOICES.map((c) => (
        <Button
          key={c}
          tier={value === c ? "primary" : "secondary"}
          size="sm"
          disabled={disabled}
          aria-pressed={value === c}
          onClick={() => onChange(c)}
        >
          {c}
        </Button>
      ))}
    </div>
  );
}

/** The engine's ask after a failed review round: one choice per finding, sent together. */
export function TriagePanel({
  cardId,
  hand,
  triage,
  nowMs,
  live,
  pending,
  refusal,
  onPress,
}: {
  cardId: string;
  hand: HandView;
  triage: TriageView;
  nowMs: number;
  live: boolean;
  pending: string | null;
  refusal: string | null;
  onPress: (offer: Offer) => void;
}) {
  const { choices, sending, sendError } = useSyncExternalStore(
    subscribe,
    () => readEntry(hand.ask),
    () => EMPTY_ENTRY,
  );
  const [proofs, setProofs] = useState<Record<number, boolean>>({});
  const cold = coldMinutes(hand.cold_at_ms, nowMs);
  const left = triage.findings.filter((f) => choices[f.n] === undefined).length;
  const count = (c: Choice) => triage.findings.filter((f) => choices[f.n] === c).length;
  const tally =
    left === triage.findings.length
      ? `${left} to choose`
      : [
          `${count("Fix")} fix`,
          `${count("Decline")} decline`,
          `${count("Foreman decides")} foreman decides`,
          ...(left > 0 ? [`${left} to choose`] : []),
        ].join(" · ");

  const send = () => {
    if (!live || left > 0 || sending) return;
    const fix: string[] = [];
    const decline: string[] = [];
    const foreman: string[] = [];
    for (const f of triage.findings) {
      const c = choices[f.n];
      if (c === "Fix") fix.push(String(f.n));
      else if (c === "Decline") decline.push(String(f.n));
      else if (c === "Foreman decides") foreman.push(String(f.n));
    }
    const ask = hand.ask;
    updateEntry(ask, { sending: true, sendError: null });
    sendCommand({ triage: { card: cardId, fix, decline, foreman } })
      .then(() => {
        triageStore.delete(ask);
        notify();
      })
      .catch((e: unknown) => {
        updateEntry(ask, { sending: false, sendError: e instanceof Error ? e.message : String(e) });
      });
  };

  return (
    <HandCard
      color={hand.color}
      head={
        <>
          <span className="text-label-13 text-gray-900">
            Review round {triage.round} {triage.stalemate ? "stalemate" : "failed"} · {triage.findings.length}{" "}
            {plural(triage.findings.length, "finding", "findings")} to triage
          </span>
          {hand.given && <span className="text-label-13 text-gray-900">sent to orchestrator</span>}
          {cold !== null && <Cold minutes={cold} />}
          <span className="font-mono text-label-12 tabular-nums text-gray-700">{ago(nowMs - hand.raised_ms)}</span>
        </>
      }
      actions={
        <>
          <OfferButtons
            offers={hand.offers}
            live={live && !sending}
            pending={pending}
            onPress={onPress}
            className="flex flex-wrap items-center gap-2"
          />
          <span className="pr-2 font-mono text-label-12 tabular-nums text-gray-900">{tally}</span>
          <Button tier="primary" size="sm" disabled={!live || left > 0 || sending} onClick={send}>
            {sending ? "Sending…" : "Send"}
          </Button>
        </>
      }
    >
      <ul className="divide-y divide-gray-400">
        {triage.findings.map((f) => (
          <li key={f.n} className="py-2 pl-4 pr-2">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              <button
                type="button"
                aria-expanded={proofs[f.n] ?? false}
                onClick={() => setProofs((p) => ({ ...p, [f.n]: !(p[f.n] ?? false) }))}
                className="flex min-w-[180px] flex-1 cursor-pointer items-start gap-3 text-left hover:text-gray-1000"
              >
                <ChevronIcon open={proofs[f.n] ?? false} className="mt-[3px] text-gray-900" />
                <span className="w-8 shrink-0 font-mono text-label-12 leading-5 text-gray-1000">#{f.n}</span>
                <span className="min-w-0 flex-1">
                  <span className="block text-label-14 text-gray-1000">{f.summary}</span>
                  <span className="mt-1 block truncate font-mono text-label-12 text-gray-700">
                    {f.file}:{f.line} · {f.by}
                  </span>
                </span>
              </button>
              <ChoiceSwitch
                value={choices[f.n]}
                disabled={sending}
                onChange={(c) => {
                  const next = { ...choices, [f.n]: c };
                  updateEntry(hand.ask, { choices: next, sendError: null });
                }}
              />
            </div>
            {(proofs[f.n] ?? false) && (
              <div className="ml-[70px] mt-2 max-w-[74ch] border-l border-gray-400 pl-3">
                <Markdown text={f.proof} className="text-gray-900" />
              </div>
            )}
          </li>
        ))}
      </ul>
      <RefusalLine refusal={sendError ?? refusal} className="px-4 py-2" />
    </HandCard>
  );
}
