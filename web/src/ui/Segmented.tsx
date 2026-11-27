export interface SegmentedProps<T extends string> {
  options: readonly T[];
  value: T;
  onChange: (value: T) => void;
  compact?: boolean;
  disabled?: (option: T) => boolean;
  label?: (option: T) => string;
}

/** Bordered segmented control with a filled active segment. */
export function Segmented<T extends string>({ options, value, onChange, label, compact = false, disabled }: SegmentedProps<T>) {
  return (
    <div className="inline-flex h-control divide-x divide-gray-400 border border-gray-400">
      {options.map((o) => (
        <button
          key={o}
          type="button"
          aria-pressed={o === value}
          disabled={disabled?.(o)}
          onClick={() => onChange(o)}
          className={`h-full cursor-pointer px-3 transition-colors disabled:cursor-not-allowed ${compact ? "text-label-12" : "text-label-13"} ${
            o === value ? "bg-gray-200 font-medium text-gray-1000" : "text-gray-900 hover:text-gray-1000 disabled:opacity-50"
          }`}
        >
          {label !== undefined ? label(o) : o}
        </button>
      ))}
    </div>
  );
}
