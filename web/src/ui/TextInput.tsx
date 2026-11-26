import type { ChangeEventHandler } from "react";

interface TextInputProps {
  id?: string;
  autoFocus?: boolean;
  value: string;
  onChange: ChangeEventHandler<HTMLInputElement>;
  placeholder?: string;
}

export function TextInput({ id, autoFocus, value, onChange, placeholder }: TextInputProps) {
  return (
    <input
      id={id}
      autoFocus={autoFocus}
      value={value}
      onChange={onChange}
      placeholder={placeholder}
      className="h-[30px] min-w-0 w-full border border-gray-400 bg-background-100 px-3 text-label-13 text-gray-1000 placeholder:text-gray-700"
    />
  );
}
