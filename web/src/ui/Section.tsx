import type { ReactNode } from "react";

export interface SectionHeadProps {
  title: string;
  note?: ReactNode;
  right?: ReactNode;
}

/** A section's heading line: its title, an optional mono count or note beside it, and a right-hand slot. */
export function SectionHead({ title, note, right }: SectionHeadProps) {
  return (
    <div className="mb-3 flex min-h-[26px] flex-wrap items-center gap-x-3 gap-y-2">
      <h2 className="text-heading-14 text-gray-1000">{title}</h2>
      {note !== undefined && <span className="font-mono text-label-12 tabular-nums text-gray-700">{note}</span>}
      {right !== undefined && <div className="ml-auto flex flex-wrap items-center gap-x-4 gap-y-2">{right}</div>}
    </div>
  );
}
