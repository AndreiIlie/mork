import type { KeyboardEvent, ReactNode } from "react";
import { Button } from "./Button";

export interface ComposerProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  placeholder: string;
  disabled?: boolean;
  sending?: boolean;
  error?: string | null;
  left?: ReactNode;
}

/** Full-bleed fused composer strip: the strip is the input, with a left slot and Send fused underneath. */
export function Composer({
  value,
  onChange,
  onSubmit,
  placeholder,
  disabled = false,
  sending = false,
  error = null,
  left,
}: ComposerProps) {
  const locked = disabled || sending;
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (!locked && value.trim() !== "") onSubmit();
    }
  };
  return (
    <div className="flex shrink-0 flex-col border-t border-gray-400 bg-background-200 transition-colors focus-within:bg-gray-100">
      <textarea
        rows={2}
        value={value}
        disabled={locked}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        className="block min-h-[52px] w-full resize-none bg-transparent px-gutter pb-2 pt-3 text-copy-14 text-gray-1000 placeholder:text-gray-700 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
      />
      <div className="flex items-center gap-2 px-gutter pb-3">
        {left}
        <span className="ml-auto">
          <Button tier="primary" size="sm" disabled={locked || value.trim() === ""} onClick={onSubmit}>
            {sending ? "Sending…" : "Send"}
          </Button>
        </span>
      </div>
      {error !== null && error !== "" && (
        <p role="alert" className="px-gutter pb-3 text-copy-13 text-red-900">
          {error}
        </p>
      )}
    </div>
  );
}
