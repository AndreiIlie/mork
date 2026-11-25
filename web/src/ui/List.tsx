import type { ReactNode } from "react";
import { PageLink } from "../route";

const INDENT = {
  0: "pl-edge",
  1: "pl-[calc(var(--spacing-edge)+24px)]",
  2: "pl-[calc(var(--spacing-edge)+48px)]",
} as const;

const PANE_INDENT = {
  0: "pl-gutter",
  1: "pl-[calc(var(--spacing-gutter)+24px)]",
  2: "pl-[calc(var(--spacing-gutter)+48px)]",
} as const;

export type RowIndent = keyof typeof INDENT;

/** A bordered list: its rows own the inner edge padding, the list has none. */
export function List({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-gray-400 border border-gray-400">{children}</div>;
}

/** A full-bleed list for a pane: hairlines run edge to edge, rows own the page gutter. */
export function PaneList({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-gray-400 border-b border-gray-400">{children}</div>;
}

export interface RowProps {
  children: ReactNode;
  indent?: RowIndent;
  pane?: boolean;
  onClick?: () => void;
  expanded?: boolean;
  to?: string;
  actions?: ReactNode;
  active?: boolean;
}

/** A row is plain, a button (`onClick`) or a link (`to`); `actions` sit beside the trigger, never inside it. */
export function Row({ children, indent = 0, pane = false, onClick, expanded, to, actions, active = false }: RowProps) {
  const left = pane ? PANE_INDENT[indent] : INDENT[indent];
  const right = pane ? "pr-gutter" : "pr-edge";
  const interactive = onClick !== undefined || to !== undefined;
  const fill = active ? "bg-gray-200" : interactive ? "transition-colors hover:bg-gray-100" : "";
  const inner = `flex min-w-0 flex-1 items-center gap-3 py-3 text-left text-label-14 ${left} ${actions === undefined ? right : ""}`;
  const trigger =
    to !== undefined ? (
      <PageLink to={to} className={inner}>
        {children}
      </PageLink>
    ) : onClick !== undefined ? (
      <button type="button" onClick={onClick} aria-expanded={expanded} className={`${inner} cursor-pointer`}>
        {children}
      </button>
    ) : (
      <div className={inner}>{children}</div>
    );
  return (
    <div className={`flex w-full items-center gap-3 ${fill} ${actions === undefined ? "" : right}`}>
      {trigger}
      {actions}
    </div>
  );
}
