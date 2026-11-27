import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { CommandHelp } from "./CommandHelp";
import { RaisedHands } from "./Hands";
import { SpendChart, money, spendKpis } from "./Usage";
import { ownHands } from "./alerts";
import { submitComposer } from "./bridge";
import { plural, workingSeats } from "./format";
import { RefusalLine } from "./offers";
import { SETTINGS_PATH, cardPath, navigate } from "./route";
import type { RecentBurn, Screen } from "./types";
import { Button } from "./ui/Button";
import { Composer } from "./ui/Composer";
import { KpiStrip } from "./ui/Kpi";
import { PageHeader, PageScroll } from "./ui/Page";

function figures(screen: Screen, burn: RecentBurn | null) {
  const rows = screen.boards.flatMap((b) => b.live.cards);
  const seats = rows.filter((r) => r.room === undefined).reduce((n, r) => n + workingSeats(r, screen.rooms).length, 0);
  const today = burn?.daily?.at(-1);
  return [
    { label: "Active Cards", value: String(rows.length), sub: `${seats} ${plural(seats, "seat", "seats")} working now` },
    {
      label: "Landed Today",
      value: today === undefined ? "?" : String(today.landed),
      sub: today === undefined ? "" : `${money(today.cents, today.unknown)} spent today`,
    },
    ...spendKpis(burn),
  ];
}

interface Result {
  line: string;
  answer: unknown;
}

function CommandResult({ result, close }: { result: Result; close: () => void }) {
  const body = result.answer;
  const empty = body == null || (typeof body === "object" && !Array.isArray(body) && Object.keys(body).length === 0);
  return (
    <section aria-label="Command result" className="flex max-h-[40vh] shrink-0 flex-col border-t border-gray-400">
      <div className="flex shrink-0 items-center gap-3 px-gutter py-2">
        <h3 className="min-w-0 truncate font-mono text-label-13 text-gray-1000">{result.line}</h3>
        {empty && <span className="font-mono text-label-12 text-gray-700">ok</span>}
        <span className="ml-auto">
          <Button tier="secondary" size="sm" title="Close result (Esc)" onClick={close}>Close</Button>
        </span>
      </div>
      {!empty && <pre className="min-h-0 overflow-auto border-t border-gray-400 px-gutter py-3 font-mono text-label-12 text-gray-900">{JSON.stringify(body, null, 2)}</pre>}
    </section>
  );
}

export function Console({ screen, burn, burnError, nowMs, live }: {
  screen: Screen;
  burn: RecentBurn | null;
  burnError: string | null;
  nowMs: number;
  live: boolean;
}) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [help, setHelp] = useState<[string, string][] | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  const active = useRef(false);
  useLayoutEffect(() => {
    active.current = true;
    return () => { active.current = false; };
  }, []);
  useEffect(() => {
    if (!result) return;
    const close = (e: KeyboardEvent) => { if (e.key === "Escape") setResult(null); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [result]);

  const submit = async () => {
    if (!live || sending || !text.trim()) return;
    setResult(null);
    if (!/^\p{White_Space}*\//u.test(text)) {
      setError("✗ the console runs commands only: /help lists them");
      return;
    }
    setError(null);
    setSending(true);
    try {
      const { action, answer } = await submitComposer({ text, context: { place: "main", card: null, to: "" }, images: [], now: false });
      if (!active.current) return;
      setText("");
      setHelp(action?.action === "help" ? action.commands : null);
      if (!action) setResult({ line: text, answer });
      else if (action.action === "usage") navigate(SETTINGS_PATH.usage);
      else if (action.action === "open") navigate(cardPath(action.card));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSending(false);
    }
  };

  const hands = ownHands(screen.hands);
  const days = burn?.daily ?? [];
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader crumbs={<span>Workspace</span>} title="Mork Console" />
      {help ? <CommandHelp main commands={help} close={() => setHelp(null)} /> : (
        <PageScroll>
          <RefusalLine refusal={burnError} className="mb-3" />
          <KpiStrip items={figures(screen, burn)} />
          {hands.length > 0 && <RaisedHands hands={hands} nowMs={nowMs} live={live} />}
          {days.length > 0 && <SpendChart days={days} tzOffsetMin={screen.tz_offset_min} />}
        </PageScroll>
      )}
      {result && <CommandResult result={result} close={() => setResult(null)} />}
      <Composer
        value={text}
        onChange={(v) => { setText(v); setError(null); }}
        onSubmit={submit}
        placeholder="Run a command… /help lists them"
        disabled={!live}
        sending={sending}
        error={error}
      />
    </main>
  );
}
