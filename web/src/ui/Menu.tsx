import { useEffect, useRef, useState } from "react";
import { Button } from "./Button";

interface Item {
  label: string;
  disabled: boolean;
  pick: () => void;
}

export function Menu({ items }: { items: Item[] }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:enabled')?.focus();
    const outside = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  const close = () => {
    setOpen(false);
    root.current?.querySelector("button")?.focus();
  };
  return (
    <div className="relative" ref={root} onBlur={(e) => {
      if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
    }} onKeyDown={(e) => {
      if (e.key === "Escape" && open) close();
      if (!open || !["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:enabled'));
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === "Home" ? 0 : e.key === "End" ? buttons.length - 1 : current + (e.key === "ArrowUp" ? -1 : 1);
      buttons[(next + buttons.length) % buttons.length]?.focus();
    }}>
      <Button tier="secondary" square aria-label="More" aria-expanded={open} aria-haspopup="menu" pressed={open} onClick={() => setOpen(!open)}>⋯</Button>
      {open && <div role="menu" aria-label="More" className="absolute right-0 top-[32px] z-10 w-[180px] border border-gray-400 bg-background-100">
        {items.map((item) => <button key={item.label} type="button" role="menuitem" disabled={item.disabled} onClick={() => { close(); item.pick(); }}
          className="flex h-control w-full cursor-pointer items-center px-3 text-left text-label-13 text-gray-1000 hover:bg-gray-100 disabled:cursor-not-allowed disabled:text-gray-700 disabled:hover:bg-background-100">{item.label}</button>)}
      </div>}
    </div>
  );
}
