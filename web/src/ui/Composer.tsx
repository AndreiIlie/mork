import { type KeyboardEvent, type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ImageThumbnail } from "./ImagePreview";
import { Button } from "./Button";

function fitHeight(el: HTMLTextAreaElement) {
  // Hide the scrollbar while measuring: its width would rewrap the text and overstate the height.
  el.style.overflowY = "hidden";
  el.style.height = "auto";
  el.style.height = `${el.scrollHeight}px`;
  el.style.overflowY = "";
}

export interface Attachment {
  id: number;
  file: File;
}

function DraftThumbnail({ file }: { file: File }) {
  const [url, setUrl] = useState("");
  useEffect(() => {
    const local = URL.createObjectURL(file);
    setUrl(local);
    return () => URL.revokeObjectURL(local);
  }, [file]);
  return url ? <ImageThumbnail url={url} name={file.name || "Pasted image"} draft /> : null;
}

export interface ComposerProps {
  attachments: Attachment[];
  onPasteImages: (files: File[]) => void;
  onRemove: (id: number) => void;
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
  attachments,
  onPasteImages,
  onRemove,
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
  const ready = !locked && (value.trim() !== "" || attachments.length > 0);
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
      if (ready) onSubmit();
    }
  };
  return (
    <div className="flex shrink-0 flex-col border-t border-gray-400 bg-background-200 transition-colors focus-within:bg-gray-100">
      {attachments.length > 0 && (
        <div className="flex max-h-[30vh] flex-wrap gap-2 overflow-y-auto px-gutter pt-3">
          {attachments.map(({ id, file }) => (
            <div key={id} className="flex max-w-full items-start">
              <DraftThumbnail file={file} />
              <Button tier="ghost" square size="sm" disabled={locked} title="Remove image"
                aria-label={`Remove ${file.name || "image"}`} onClick={() => onRemove(id)}>×</Button>
            </div>
          ))}
        </div>
      )}
      <label className="block cursor-text px-gutter pb-3 pt-3">
        <textarea
          ref={box}
          rows={2}
          value={value}
          disabled={locked}
          onChange={(e) => onChange(e.target.value)}
          onPaste={(event) => {
            if (locked || event.clipboardData.getData("text/plain") !== "") return;
            const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
            if (!images.length) return;
            event.preventDefault();
            onPasteImages(images);
          }}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          aria-label={placeholder}
          className="block max-h-[160px] w-full resize-none overflow-y-auto bg-transparent p-0 text-copy-14 text-gray-1000 placeholder:text-gray-700 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
        />
      </label>
      <div className="flex items-center gap-2 px-gutter pb-3">
        {left}
        <span className="ml-auto flex items-center gap-2">
          <Button tier="secondary" size="sm" disabled={!ready} onClick={onSendNow}
            title="Send and interrupt the recipient’s running turn">Send now</Button>
          <Button tier="primary" size="sm" disabled={!ready} onClick={onSubmit}>
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
