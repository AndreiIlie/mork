import type { ChangeEventHandler } from "react";

interface TextInputProps {
  id?: string;
  label?: string;
  width?: "sm" | "md" | "fill";
  autoFocus?: boolean;
  value: string;
  onChange: ChangeEventHandler<HTMLInputElement>;
  placeholder?: string;
}

export function TextInput({ id, label, width, autoFocus, value, onChange, placeholder }: TextInputProps) {
  return (
    <input
      id={id}
      aria-label={label}
      autoFocus={autoFocus}
      value={value}
      onChange={onChange}
      placeholder={placeholder}
      className={`h-[30px] min-w-0 border border-gray-400 bg-background-100 px-3 text-label-13 text-gray-1000 placeholder:text-gray-700 ${width ? "font-mono tabular-nums" : ""} ${width === "sm" ? "w-[88px]" : width === "md" ? "w-[128px]" : width === "fill" ? "flex-1" : "w-full"}`}
    />
  );
}
