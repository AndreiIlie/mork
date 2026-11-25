import { useEffect, useRef, useState } from "react";
import { sendCommand } from "./bridge";
import { agentSeatName, modelTakesEffort } from "./format";
import { RefusalLine } from "./offers";
import type { AgentRow, ModelChoice } from "./types";

const VENDOR: Record<string, string> = { claude: "Claude", codex: "Codex", agy: "agy" };
const SEGMENTS = "inline-flex flex-wrap divide-x divide-gray-400 border border-gray-400";
const LEFT = "w-[64px] shrink-0 text-label-12 text-gray-700";

function Segment({
  on,
  label,
  disabled,
  onPick,
}: {
  on: boolean;
  label: string;
  disabled: boolean;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      disabled={disabled}
      onClick={onPick}
      className={`h-8 cursor-pointer whitespace-nowrap px-3 text-label-13 transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
        on ? "bg-gray-100 text-gray-1000" : "text-gray-900 hover:text-gray-1000"
      }`}
    >
      {label}
    </button>
  );
}

function useEscape(active: boolean, onEscape: () => void) {
  useEffect(() => {
    if (!active) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onEscape();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [active, onEscape]);
}

function usePickSender(agent: string | null, live: boolean, onClearDraft: () => void) {
  const [sending, setSending] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const agentRef = useRef<string | null>(agent);

  useEffect(() => {
    agentRef.current = agent;
  }, [agent]);

  const sendPick = (command: unknown, clearDraft: boolean) => {
    if (!live || sending || agent === null) return;
    const target = agent;
    setSending(true);
    setRefusal(null);
    sendCommand(command)
      .then(() => {
        if (clearDraft && agentRef.current === target) onClearDraft();
      })
      .catch((e: unknown) => {
        if (agentRef.current === target) setRefusal(e instanceof Error ? e.message : String(e));
      })
      .finally(() => setSending(false));
  };

  return { sending, refusal, setRefusal, sendPick };
}

/** State and `pick` command handling for the dock's folded model and effort strip. */
export function useSeatModel(seat: AgentRow | undefined, live: boolean) {
  const agent = seat?.agent ?? null;
  const [prevAgent, setPrevAgent] = useState<string | null>(agent);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<ModelChoice | null>(null);
  const { sending, refusal, setRefusal, sendPick } = usePickSender(agent, live, () => setDraft(null));

  if (prevAgent !== agent) {
    setPrevAgent(agent);
    setOpen(false);
    setDraft(null);
    setRefusal(null);
  }

  useEscape(open, () => {
    setOpen(false);
    setDraft(null);
    setRefusal(null);
  });

  const toggle = () => {
    setOpen((v) => !v);
    setDraft(null);
    setRefusal(null);
  };
  const next = seat?.pending ?? seat?.setting;
  const choices = seat?.choices ?? [];

  const pickModel = (c: ModelChoice) => {
    if (seat === undefined || next === undefined) return;
    if (!modelTakesEffort(c.cli, c.model, next.effort)) {
      setRefusal(null);
      setDraft(c);
      return;
    }
    sendPick({ pick: { agent: seat.agent, model: c.model, cli: c.cli } }, true);
  };
  const pickEffort = (effort: string) => {
    if (seat === undefined) return;
    const body = draft !== null ? { agent: seat.agent, model: draft.model, cli: draft.cli, effort } : { agent: seat.agent, effort };
    sendPick({ pick: body }, draft !== null);
  };

  return { seat, next, choices, open, toggle, draft, sending, refusal, live, pickModel, pickEffort };
}

export type SeatModelState = ReturnType<typeof useSeatModel>;

function VendorRow({
  cli,
  choices,
  lit,
  fresh,
  disabled,
  onPick,
}: {
  cli: string;
  choices: ModelChoice[];
  lit: ModelChoice;
  fresh: boolean;
  disabled: boolean;
  onPick: (c: ModelChoice) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className={LEFT}>{VENDOR[cli] ?? cli}</span>
      <div className={SEGMENTS}>
        {choices.map((c) => (
          <Segment
            key={`${c.cli}:${c.model}`}
            on={c.cli === lit.cli && c.model === lit.model}
            label={c.label}
            disabled={disabled}
            onPick={() => onPick(c)}
          />
        ))}
      </div>
      {fresh && <span className="text-label-12 text-gray-700">starts a fresh session</span>}
    </div>
  );
}

/** Unfolded strip over the composer: segmented model rows per vendor, valid efforts, and next-turn summary. */
export function SeatModelPanel({ m }: { m: SeatModelState }) {
  const { seat, next, choices, draft } = m;
  if (!m.open || seat === undefined || next === undefined || choices.length === 0) return null;
  const name = agentSeatName(seat.agent);
  const runs = seat.setting ?? next;
  const clis = [...new Set(choices.map((c) => c.cli))].sort(
    (x, y) => Number(x !== runs.model.cli) - Number(y !== runs.model.cli),
  );
  const lit = draft ?? next.model;
  const efforts = (seat.efforts ?? []).filter((e) => modelTakesEffort(lit.cli, lit.model, e));
  const same = runs.model.cli === next.model.cli && runs.model.model === next.model.model && runs.effort === next.effort;
  const foot =
    draft !== null
      ? `${draft.label} takes no ${next.effort}: choose an effort.`
      : same
        ? `${name} runs ${runs.model.label} · ${runs.effort}. A change applies from its next turn.`
        : `${name} runs ${runs.model.label} · ${runs.effort} now, and ${next.model.label} · ${next.effort} from its next turn.`;
  const disabled = !m.live || m.sending;

  return (
    <div className="border-t border-gray-400 px-gutter py-3">
      <div className="flex flex-col gap-2">
        {clis.map((cli) => (
          <VendorRow
            key={cli}
            cli={cli}
            choices={choices.filter((c) => c.cli === cli)}
            lit={lit}
            fresh={cli !== runs.model.cli}
            disabled={disabled}
            onPick={m.pickModel}
          />
        ))}
        <div className="flex flex-wrap items-center gap-3">
          <span className={LEFT}>Effort</span>
          <div className={SEGMENTS}>
            {efforts.map((e) => (
              <Segment
                key={e}
                on={draft === null && e === next.effort}
                label={e}
                disabled={disabled}
                onPick={() => m.pickEffort(e)}
              />
            ))}
          </div>
        </div>
      </div>
      <p className="mt-3 text-label-12 text-gray-900">{foot}</p>
      <RefusalLine refusal={m.refusal} className="mt-2" />
    </div>
  );
}
