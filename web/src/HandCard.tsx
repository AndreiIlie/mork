import type { ReactNode } from "react";
import type { Color, HandView } from "./types";

// The one place the card carries colour: its header's pill, in the hand's colour.
const PILL: Record<Color, string> = {
  red: "bg-red-700",
  yellow: "bg-amber-700",
  green: "bg-gray-500",
  grey: "bg-gray-500",
};

/** The colour a hand's label shows: grey once sent to an orchestrator, no longer waiting on the operator. */
export function labelColor(hand: HandView): Color {
  return hand.given ? "grey" : hand.color;
}

/** One open ask in the Raised Hands dock: a monochrome surface, its colour only in the header's pill. */
export function HandCard({
  color,
  head,
  actions,
  children,
}: {
  color: Color;
  head: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="px-gutter pt-3">
      <section className="flex flex-col border border-gray-500 bg-gray-100">
        <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-gray-400 py-2 pl-4 pr-2">
          <h2 className={`flex h-5 items-center px-2 text-label-12 font-medium text-background-100 ${PILL[color]}`}>
            Raised Hand
          </h2>
          {head}
          {actions !== undefined && <div className="ml-auto flex flex-wrap items-center gap-2">{actions}</div>}
        </div>
        {children}
      </section>
    </div>
  );
}
