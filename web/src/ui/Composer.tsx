import { type KeyboardEvent, type ReactNode, useEffect, useLayoutEffect, useRef } from "react";
import { Button } from "./Button";

function fitHeight(el: HTMLTextAreaElement) {
  // Hide the scrollbar while measuring: its width would rewrap the text and overstate the height.
  el.style.overflowY = "hidden";
  el.style.height = "auto";
  el.style.height = `${el.scrollHeight}px`;
  el.style.overflowY = "";
}

export interface ComposerProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onSendNow: () => void;
  placeholder: string;
  disabled?: boolean;
  sending?: boolean;
  error?: string | null;
  left?: ReactNode;
}

/** Full-bleed fused composer strip: the input grows from 2 to 8 lines then scrolls inside fixed padding, with a left slot and Send fused underneath. */
export function Composer({
  value,
  onChange,
  onSubmit,
  onSendNow,
  placeholder,
  disabled = false,
  sending = false,
  error = null,
  left,
}: ComposerProps) {
  const locked = disabled || sending;
  const box = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    if (box.current !== null) fitHeight(box.current);
  }, [value]);
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    let width = -1;
    // A narrower or wider pane rewraps an unchanged draft, so refit on width changes only,
    // read unrounded: a half-pixel change can rewrap a line.
    const watch = new ResizeObserver(([entry]) => {
      if (entry === undefined || entry.contentRect.width === width) return;
      width = entry.contentRect.width;
      fitHeight(el);
    });
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (!locked && value.trim() !== "") onSubmit();
    }
  };
  return (
    <div className="flex shrink-0 flex-col border-t border-gray-400 bg-background-200 transition-colors focus-within:bg-gray-100">
      <label className="block cursor-text px-gutter pb-3 pt-3">
        <textarea
          ref={box}
          rows={2}
          value={value}
          disabled={locked}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          aria-label={placeholder}
          className="block max-h-[160px] w-full resize-none overflow-y-auto bg-transparent p-0 text-copy-14 text-gray-1000 placeholder:text-gray-700 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
        />
      </label>
      <div className="flex items-center gap-2 px-gutter pb-3">
        {left}
        <span className="ml-auto flex items-center gap-2">
          <Button tier="secondary" size="sm" disabled={locked || value.trim() === ""} onClick={onSendNow}
            title="Send and interrupt the recipient’s running turn">Send now</Button>
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
