import { useEffect } from "react";
import { Button } from "./ui/Button";

const usage = [
  ["/help", "Show this list"],
  ["/open <card>", "Open a Card Room"],
  ["/usage", "Open Settings Usage"],
  ["@name message", "Send a message to that seat"],
  ["Enter", "Send"],
  ["Shift+Enter", "Insert a new line"],
];

function Lines({ rows }: { rows: string[][] }) {
  return <dl className="divide-y divide-gray-400 border-y border-gray-400">{rows.map(([label, words]) => (
    <div key={label} className="grid grid-cols-[minmax(0,9rem)_minmax(0,1fr)] gap-4 py-2">
      <dt className="font-mono text-label-13 [overflow-wrap:anywhere]">{label}</dt>
      <dd className="text-copy-13 text-gray-900 [overflow-wrap:anywhere]">{words}</dd>
    </div>
  ))}</dl>;
}

export function CommandHelp({ commands, close, main = false }: { commands: [string, string][]; close: () => void; main?: boolean }) {
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [close]);
  return <section aria-label="Command help" className="flex min-h-0 flex-1 flex-col">
    <div className="flex items-center justify-between border-b border-gray-400 px-gutter py-2">
      <h3 className="font-mono text-heading-14">/help</h3>
      <Button tier="secondary" size="sm" title="Close help (Esc)" onClick={close}>Close</Button>
    </div>
    <div className="min-h-0 overflow-y-auto px-gutter py-4">
      <Lines rows={main ? usage.filter(([label]) => !label.startsWith("@")) : usage} />
      <h4 className={main ? "mt-gutter mb-3 text-heading-14" : "mt-gutter text-heading-14"}>Operator commands</h4>
      {!main && <p className="mb-3 mt-1 text-copy-13 text-gray-900">The current conversation supplies the place and card when applicable.</p>}
      <Lines rows={commands} />
    </div>
  </section>;
}
