export interface SegmentedProps<T extends string> {
  options: readonly T[];
  value: T;
  onChange: (value: T) => void;
  label?: (option: T) => string;
}

/** Bordered segmented control with a filled active segment. */
export function Segmented<T extends string>({ options, value, onChange, label }: SegmentedProps<T>) {
  return (
    <div className="inline-flex divide-x divide-gray-400 border border-gray-400">
      {options.map((o) => (
        <button
          key={o}
          type="button"
          aria-pressed={o === value}
          onClick={() => onChange(o)}
          className={`h-7 cursor-pointer px-3 text-label-13 transition-colors ${
            o === value ? "bg-gray-200 font-medium text-gray-1000" : "text-gray-900 hover:text-gray-1000"
          }`}
        >
          {label !== undefined ? label(o) : o}
        </button>
      ))}
    </div>
  );
}
