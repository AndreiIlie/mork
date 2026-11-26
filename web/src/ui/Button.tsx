import type { AriaAttributes, MouseEventHandler, ReactNode } from "react";

const BASE =
  "inline-flex cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap border font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50";

const SIZE = {
  control: "h-7 px-3 text-label-13",
  md: "h-[30px] px-3 text-label-13",
  sm: "h-[26px] px-[10px] text-label-12",
} as const;

const SQUARE = {
  control: "h-7 w-7 text-label-13",
  md: "h-[30px] w-[30px] text-label-13",
  sm: "h-[26px] w-[26px] text-label-12",
} as const;

const TIER = {
  primary: "border-gray-1000 bg-gray-1000 text-background-100 hover:border-gray-950 hover:bg-gray-950",
  secondary: "border-gray-400 bg-background-100 text-gray-1000 hover:border-gray-500 hover:bg-gray-100",
  ghost: "border-transparent text-gray-900 hover:bg-gray-100 hover:text-gray-1000",
} as const;

export type ButtonTier = keyof typeof TIER;
export type ButtonSize = keyof typeof SIZE;

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

/** Symmetric sharp-corner button in three tiers and three sizes, optionally square. */
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
  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      title={title}
      {...aria}
      aria-pressed={ariaPressed}
      className={`${BASE} ${square ? SQUARE[size] : SIZE[size]} ${tone}`}
    >
      {children}
    </button>
  );
}
