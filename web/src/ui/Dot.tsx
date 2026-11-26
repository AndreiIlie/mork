import { DOT } from "../format";
import type { Color } from "../types";

/** The one circular primitive: an 8px status dot in the four status colours. */
export function Dot({ color, pulse = false }: { color: Color; pulse?: boolean }) {
  return <span className={`h-2 w-2 shrink-0 rounded-full ${DOT[color]} ${pulse ? "animate-pulse-soft" : ""}`} />;
}

// Red and yellow wait on the operator and outrank working; green shows only while a seat works, grey otherwise.
/** Card status dot derived from its hand light and working state. */
export function StatusDot({ light, working = false }: { light: Color | null; working?: boolean }) {
  if (light === "red" || light === "yellow") return <Dot color={light} />;
  return <Dot color={working ? "green" : "grey"} pulse={working} />;
}
