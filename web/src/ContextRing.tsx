import { thousands } from "./format";
import type { AgentRow } from "./types";

export function ContextRing({ context }: { context: AgentRow["context"] }) {
  if (context === null || context[1] <= 0) return null;
  const [used, capacity] = context;
  const label = `${thousands(used)}/${thousands(capacity)}`;
  const fill = Math.min(1, Math.max(0, used / capacity));
  return (
    <span
      role="img"
      aria-label={`Context ${label}`}
      title={label}
      className="context-ring"
      style={{ background: `conic-gradient(var(--color-gray-950) ${fill}turn, var(--color-gray-500) 0)` }}
    />
  );
}
