import type { ReactNode } from "react";

export interface SplitPaneProps {
  left: ReactNode;
  right: ReactNode;
}

/** Two equal panes separated by a vertical hairline, each scrolling independently. */
export function SplitPane({ left, right }: SplitPaneProps) {
  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      <div className="flex min-w-0 flex-1 basis-1/2 flex-col overflow-hidden">{left}</div>
      <div className="flex min-w-0 flex-1 basis-1/2 flex-col overflow-hidden border-l border-gray-400">{right}</div>
    </div>
  );
}
