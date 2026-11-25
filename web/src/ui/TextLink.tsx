import type { MouseEventHandler, ReactNode } from "react";
import { PageLink } from "../route";

const STYLE =
  "inline-flex cursor-pointer items-center gap-1 text-label-13 text-gray-900 underline-offset-[3px] hover:text-gray-1000 hover:underline";

export interface TextLinkProps {
  to?: string;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  disabled?: boolean;
  title?: string;
  children: ReactNode;
}

/** Plain text link or action: no box or padding, underline on hover. */
export function TextLink({ to, onClick, disabled, title, children }: TextLinkProps) {
  if (to !== undefined) {
    return (
      <PageLink to={to} className={STYLE}>
        {children}
      </PageLink>
    );
  }
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`${STYLE} disabled:cursor-not-allowed disabled:opacity-50`}
    >
      {children}
    </button>
  );
}
