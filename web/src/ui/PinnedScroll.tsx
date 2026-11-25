import { type ReactNode, type RefObject, useLayoutEffect, useRef, useState } from "react";
import { ArrowDownIcon } from "../icons";
import { Button } from "./Button";

const BOTTOM_SLACK_PX = 24;

function atBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK_PX;
}

export interface PinnedScrollProps {
  children: ReactNode;
  /** Receives the scroll box, for a caller that restores the reader's place after prepending rows. */
  scrollRef?: RefObject<HTMLDivElement | null>;
  /** Set by a caller that moved the scroll box to restore the reader's place: the next resize keeps that place. */
  keepRef?: RefObject<boolean>;
}

/** A pane's scroll area that stays at its bottom while the reader is there, and offers a jump back while scrolled up. */
export function PinnedScroll({ children, scrollRef, keepRef }: PinnedScrollProps) {
  const ownRef = useRef<HTMLDivElement | null>(null);
  const boxRef = scrollRef ?? ownRef;
  const contentRef = useRef<HTMLDivElement | null>(null);
  const pinnedRef = useRef(true);
  const lastTopRef = useRef(0);
  const clickedRef = useRef(false);
  const [away, setAway] = useState(false);

  const setPinned = (value: boolean) => {
    if (pinnedRef.current === value) return;
    pinnedRef.current = value;
    setAway(!value);
  };

  useLayoutEffect(() => {
    const box = boxRef.current;
    const content = contentRef.current;
    if (box === null || content === null || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(() => {
      if (box.clientHeight === 0) return;
      const keep = clickedRef.current || keepRef?.current === true;
      if (keepRef !== undefined) keepRef.current = false;
      // The reader's own unfold or a restored place stays put, and an unpinned pane pins again once at its bottom.
      if (keep || !pinnedRef.current) setPinned(atBottom(box));
      else box.scrollTop = box.scrollHeight;
      lastTopRef.current = box.scrollTop;
    });
    observer.observe(box);
    observer.observe(content);
    return () => observer.disconnect();
  }, [boxRef, keepRef]);

  const onScroll = () => {
    const box = boxRef.current;
    if (box === null || box.clientHeight === 0) return;
    const up = box.scrollTop < lastTopRef.current;
    lastTopRef.current = box.scrollTop;
    // A scroll event unpins only going up: it can land after rows grew and before the observer re-pins them.
    if (atBottom(box)) setPinned(true);
    else if (up) setPinned(false);
  };

  // Held for two frames so the observer, which runs after the next frame's layout, sees the click.
  const onClickCapture = () => {
    clickedRef.current = true;
    requestAnimationFrame(() => requestAnimationFrame(() => (clickedRef.current = false)));
  };

  const toBottom = () => {
    const box = boxRef.current;
    if (box === null) return;
    box.scrollTop = box.scrollHeight;
    lastTopRef.current = box.scrollTop;
    setPinned(true);
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div ref={boxRef} onScroll={onScroll} onClickCapture={onClickCapture} className="min-h-0 flex-1 overflow-y-auto">
        <div ref={contentRef}>{children}</div>
      </div>
      {away && (
        <div className="pointer-events-none absolute inset-x-0 bottom-3 flex justify-center">
          <span className="pointer-events-auto">
            <Button tier="secondary" size="sm" square aria-label="Scroll to bottom" title="Scroll to bottom" onClick={toBottom}>
              <ArrowDownIcon />
            </Button>
          </span>
        </div>
      )}
    </div>
  );
}
