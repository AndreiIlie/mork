import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "./Button";

type Point = { x: number; y: number };
type View = Point & { scale: number };
const CENTER = { x: 0, y: 0 };
const MAX_SCALE = 8;

function clamp(value: number, low: number, high: number) {
  return Math.min(high, Math.max(low, value));
}

function relativePoint(stage: HTMLElement, x: number, y: number): Point {
  const rect = stage.getBoundingClientRect();
  return { x: x - rect.left - rect.width / 2, y: y - rect.top - rect.height / 2 };
}

/** Opens the same Card Room preview for local draft images and fetched message images. */
export function ImageThumbnail({ url, name, draft = false }: { url: string; name: string; draft?: boolean }) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={button} type="button" className="block min-w-0 max-w-full cursor-zoom-in" title={name}
        aria-label={`Preview ${name}`} aria-haspopup="dialog" onClick={() => setOpen(true)}>
        <img src={url} alt={name}
          className={`block w-auto max-w-full border border-gray-400 object-contain ${draft ? "h-16" : "h-24"}`} />
      </button>
      {open && <ImagePreview url={url} name={name} onClose={() => {
        setOpen(false);
        button.current?.focus({ preventScroll: true });
      }} />}
    </>
  );
}

function ImagePreview({ url, name, onClose }: { url: string; name: string; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const pointers = useRef(new Map<number, Point>());
  const [view, setView] = useState<View>({ ...CENTER, scale: 1 });
  const [fit, setFit] = useState(1);

  function fitImage() {
    const area = stage.current, picture = image.current;
    if (!area || !picture?.naturalWidth || !area.clientHeight) return;
    const width = Math.max(1, area.clientWidth - 48);
    const height = Math.max(1, area.clientHeight - 48);
    const scale = Math.min(1, width / picture.naturalWidth, height / picture.naturalHeight);
    setFit(scale);
    setView({ ...CENTER, scale });
  }

  useLayoutEffect(() => {
    const modal = dialog.current!;
    const room = modal.closest("main") ?? document.documentElement;
    const resize = () => {
      const rect = room.getBoundingClientRect();
      Object.assign(modal.style, {
        left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px`,
      });
      fitImage();
    };
    modal.showModal();
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(room);
    return () => { observer.disconnect(); modal.close(); };
  }, []);

  function close() {
    dialog.current?.close();
    onClose();
  }

  function transform(factor: number, from = CENTER, to = from) {
    const area = stage.current, picture = image.current;
    if (!area || !picture?.naturalWidth) return;
    setView((old) => {
      const scale = clamp(old.scale * factor, fit, MAX_SCALE);
      const ratio = scale / old.scale;
      const limitX = Math.max(0, (picture.naturalWidth * scale - area.clientWidth) / 2);
      const limitY = Math.max(0, (picture.naturalHeight * scale - area.clientHeight) / 2);
      // Keep the pixel under the pointer (or pinch midpoint) there when zooming.
      return { scale, x: clamp(to.x + (old.x - from.x) * ratio, -limitX, limitX),
        y: clamp(to.y + (old.y - from.y) * ratio, -limitY, limitY) };
    });
  }

  useEffect(() => {
    const area = stage.current!;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? area.clientHeight : 1;
      const factor = Math.exp(clamp(-event.deltaY * unit * 0.002, -1, 1));
      transform(factor, relativePoint(area, event.clientX, event.clientY));
    };
    area.addEventListener("wheel", wheel, { passive: false });
    return () => area.removeEventListener("wheel", wheel);
  }, [fit]);

  function move(id: number, point: Point) {
    const active = pointers.current;
    const last = active.get(id);
    if (!last) return;
    active.set(id, point);
    const other = [...active].find(([key]) => key !== id)?.[1];
    if (!other) {
      transform(1, last, point);
      return;
    }
    const distance = (p: Point) => Math.hypot(p.x - other.x, p.y - other.y);
    const middle = (p: Point) => ({ x: (p.x + other.x) / 2, y: (p.y + other.y) / 2 });
    const before = distance(last);
    if (before > 0) transform(distance(point) / before, middle(last), middle(point));
  }

  return (
    <dialog ref={dialog} aria-label={`Preview ${name}`} onCancel={(event) => { event.preventDefault(); close(); }}
      className="fixed inset-auto m-0 max-h-none max-w-none border-0 bg-background-100 p-0 text-gray-1000 backdrop:bg-background-100/65">
      <div className="flex h-full flex-col">
        <header className="flex shrink-0 items-center gap-2 border-b border-gray-400 bg-background-200 px-gutter py-3">
          <span className="mr-auto min-w-0 truncate font-mono text-label-13" title={name}>{name}</span>
          <div className="flex items-center gap-1">
            <Button tier="secondary" size="sm" square aria-label="Zoom out" title="Zoom out"
              disabled={view.scale <= fit} onClick={() => transform(0.8)}>−</Button>
            <output className="w-12 text-center font-mono text-label-12 tabular-nums text-gray-900">
              {Math.round(view.scale * 100)}%
            </output>
            <Button tier="secondary" size="sm" square aria-label="Zoom in" title="Zoom in"
              disabled={view.scale >= MAX_SCALE} onClick={() => transform(1.25)}>+</Button>
          </div>
          <Button tier="secondary" size="sm" title="Fit image to view" onClick={fitImage}>Fit</Button>
          <Button tier="ghost" size="sm" square aria-label="Close preview" title="Close preview (Esc)" onClick={close}>
            <span className="text-heading-16">×</span>
          </Button>
        </header>
        <div ref={stage} className={`relative min-h-0 flex-1 touch-none select-none overflow-hidden bg-gray-100 ${view.scale > fit ? "cursor-grab active:cursor-grabbing" : ""}`}
          onPointerDown={(event) => {
            if (event.button !== 0 || pointers.current.size >= 2) return;
            pointers.current.set(event.pointerId, relativePoint(event.currentTarget, event.clientX, event.clientY));
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onPointerMove={(event) => {
            if (pointers.current.has(event.pointerId)) move(event.pointerId, relativePoint(event.currentTarget, event.clientX, event.clientY));
          }}
          onLostPointerCapture={(event) => pointers.current.delete(event.pointerId)}>
          <img ref={image} src={url} alt={name} draggable={false} onLoad={fitImage}
            className="pointer-events-none absolute left-1/2 top-1/2 max-w-none"
            style={{ transform: `translate(-50%, -50%) translate(${view.x}px, ${view.y}px) scale(${view.scale})` }} />
        </div>
      </div>
    </dialog>
  );
}
