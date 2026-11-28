import type { ReactNode } from "react";
import { PageHeader, PageScroll } from "../ui/Page";

export function SettingsPage({ crumb, title, words, right, children }: {
  crumb?: string;
  title: string;
  words: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  const crumbs = <><span>Settings</span>{crumb && <><span className="text-gray-700">/</span><span>{crumb}</span></>}</>;
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader crumbs={crumbs} title={title} description={words} right={right} />
      <PageScroll><div className="max-w-[960px]">{children}</div></PageScroll>
    </main>
  );
}
