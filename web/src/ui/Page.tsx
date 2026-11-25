import type { ReactNode } from "react";

export interface PageHeaderProps {
  crumbs: ReactNode;
  title: string;
  right?: ReactNode;
  children?: ReactNode;
}

/** Full-width page header with the 24px gutter, breadcrumbs row, right slot, title, and optional sub-rows. */
export function PageHeader({ crumbs, title, right, children }: PageHeaderProps) {
  return (
    <header className="shrink-0 border-b border-gray-400 px-gutter py-4">
      <div className="flex items-center gap-2 text-label-13 text-gray-900">
        {crumbs}
        {right !== undefined && <div className="ml-auto flex items-center gap-[10px]">{right}</div>}
      </div>
      <h1 className="mt-1.5 truncate text-heading-24 text-gray-1000">{title}</h1>
      {children}
    </header>
  );
}

/** Full-width scrollable page body padded with the 24px gutter. */
export function PageScroll({ children }: { children: ReactNode }) {
  return <div className="min-h-0 flex-1 overflow-y-auto p-gutter">{children}</div>;
}
