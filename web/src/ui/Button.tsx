import type { AriaAttributes, MouseEventHandler, ReactNode } from "react";

const BASE =
  "inline-flex cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap border font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50";

const TIER = {
  primary: "border-gray-1000 bg-gray-1000 text-background-100 hover:border-gray-950 hover:bg-gray-950",
  secondary: "border-gray-400 bg-background-100 text-gray-1000 hover:border-gray-500 hover:bg-gray-100",
  ghost: "border-transparent text-gray-900 hover:bg-gray-100 hover:text-gray-1000",
} as const;

export type ButtonTier = keyof typeof TIER;
export type ButtonSize = "sm" | "md";

export interface ButtonProps extends AriaAttributes {
  tier: ButtonTier;
  size?: ButtonSize;
  pressed?: boolean;
  /** Width equal to height with no side padding, for an icon alone. */
  square?: boolean;
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  title?: string;
  children: ReactNode;
}

const PRESSED = "border-gray-1000 bg-gray-200 text-gray-1000";

/** Symmetric sharp-corner button in three tiers with a shared height, optionally square. */
export function Button({
  tier,
  size = "md",
  pressed,
  square = false,
  type = "button",
  disabled,
  onClick,
  title,
  children,
  ...aria
}: ButtonProps) {
  const tone = pressed === true && tier === "secondary" ? PRESSED : TIER[tier];
  const ariaPressed = aria["aria-pressed"] ?? pressed;
  const width = square ? "w-control" : size === "sm" ? "px-[10px]" : "px-3";
  const label = size === "sm" ? "text-label-12" : "text-label-13";
  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      title={title}
      {...aria}
      aria-pressed={ariaPressed}
      className={`${BASE} h-control ${width} ${label} ${tone}`}
    >
      {children}
    </button>
  );
}
