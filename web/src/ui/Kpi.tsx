export interface Kpi {
  label: string;
  value: string;
  sub: string;
}

/** The strip of figures that heads Board and Usage: one bordered row of four cells, two by two when narrow. */
export function KpiStrip({ items }: { items: Kpi[] }) {
  return (
    <div className="grid grid-cols-2 border border-gray-400 lg:grid-cols-4">
      {items.map((k, i) => (
        <div
          key={k.label}
          className={`min-w-0 p-edge ${i > 0 ? "lg:border-l lg:border-gray-400" : ""} ${i % 2 === 1 ? "max-lg:border-l max-lg:border-gray-400" : ""} ${i > 1 ? "max-lg:border-t max-lg:border-gray-400" : ""}`}
        >
          <div className="text-label-12 text-gray-900">{k.label}</div>
          <div className="mt-1.5 font-mono text-heading-24 tabular-nums text-gray-1000">{k.value}</div>
          <div className="mt-1 text-label-12 text-gray-700">{k.sub}</div>
        </div>
      ))}
    </div>
  );
}
