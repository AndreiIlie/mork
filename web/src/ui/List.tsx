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

const DIVIDERS = "divide-y divide-gray-400";
const TABLE_CELL = "whitespace-nowrap px-3 first:pl-edge last:pr-edge";

export interface TableColumn {
  label: string;
  kind?: "identity" | "number";
}

interface TableHead {
  label: string;
  columns: readonly TableColumn[];
}

/** A bordered list whose tabular rows share column widths with their header. */
export function List({ children, table }: { children: ReactNode; table?: TableHead }) {
  return (
    <div className={`${DIVIDERS} border border-gray-400`}>
      {table ? (
        <div className="overflow-x-auto">
          <table aria-label={table.label} className="w-full text-left text-label-13">
            <thead>
              <tr>{table.columns.map(({ label, kind }) => (
                <th key={label} scope="col" className={`${TABLE_CELL} py-2 font-normal text-gray-700 ${kind === "number" ? "text-right" : ""} ${kind === "identity" ? "w-[300px]" : ""}`}>{label}</th>
              ))}</tr>
            </thead>
            <tbody className={`${DIVIDERS} border-t border-gray-400`}>{children}</tbody>
          </table>
        </div>
      ) : children}
    </div>
  );
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
export function Row(props: RowProps | { cells: readonly ReactNode[]; columns: readonly TableColumn[] }) {
  if ("cells" in props) {
    return (
      <tr>{props.cells.map((cell, i) => (
        <td key={i} className={`${TABLE_CELL} py-3 ${props.columns[i].kind === "number" ? "text-right font-mono tabular-nums" : ""}`}>{cell}</td>
      ))}</tr>
    );
  }
  const { children, indent = 0, pane = false, onClick, expanded, to, actions, active = false } = props;
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
