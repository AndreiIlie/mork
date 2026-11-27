export function MorkMark() {
  return (
    <svg viewBox="2 5 20 15" className="h-[15px] w-5 shrink-0 text-gray-1000" fill="currentColor" aria-hidden="true">
      <path d="M2 20V5L12 12V5L22 12V20H18.5V13.8L12 9.2V16.2L5.5 11.6V20H2Z" />
    </svg>
  );
}

export function HandIcon({ className = "" }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={`h-3.5 w-3.5 shrink-0 ${className}`}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.9"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
      <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
      <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
      <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
    </svg>
  );
}

export function SettingsIcon() {
  return (
    <svg viewBox="0 0 16 16" className="h-4 w-4 shrink-0" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path d="M2 4.5h6M12 4.5h2M2 11.5h2M8 11.5h6" strokeLinecap="square" />
      <rect x="8.75" y="2.75" width="3.5" height="3.5" />
      <rect x="4.75" y="9.75" width="3.5" height="3.5" />
    </svg>
  );
}

export function ChevronIcon({ open, className = "" }: { open: boolean; className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={`h-3.5 w-3.5 shrink-0 transition-transform duration-150 ${open ? "rotate-90" : ""} ${className}`}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M6 4l4 4-4 4" />
    </svg>
  );
}

export function ArrowDownIcon() {
  return (
    <svg
      viewBox="0 0 16 16"
      className="h-3.5 w-3.5 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M8 3v10M3.5 8.5L8 13l4.5-4.5" />
    </svg>
  );
}

/** The one coloured text beside an error: a prompt cache's last ten minutes. */
export function Cold({ minutes }: { minutes: number }) {
  return <span className="shrink-0 font-mono text-label-12 tabular-nums text-amber-900">cold in {minutes}m</span>;
}
