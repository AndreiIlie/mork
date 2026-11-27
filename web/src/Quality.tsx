import type { ReactNode } from "react";
import { dollars } from "./format";
import { useQuality } from "./quality";
import { SectionHead } from "./ui/Section";

type Cell = string | number;

function Figures({ title, population, columns, rows, empty }: {
  title: string;
  population: string;
  columns: string[];
  rows: Cell[][];
  empty: ReactNode;
}) {
  return (
    <section className="mb-gutter">
      <SectionHead title={title} note={population} />
      {rows.length === 0 ? <p className="text-label-13 text-gray-700">{empty}</p> : (
        <div className="overflow-x-auto border border-gray-400">
          <table className="w-full text-left text-label-13" aria-label={title}>
            <thead>
              <tr>{columns.map(label => <th scope="col" className="whitespace-nowrap px-3 py-2 text-gray-700" key={label}>{label}</th>)}</tr>
            </thead>
            <tbody className="divide-y divide-gray-400">
              {rows.map(row => <tr key={JSON.stringify(row)}>
                {row.map((cell, i) => <td className="whitespace-nowrap px-3 py-3 tabular-nums" key={columns[i]}>{cell}</td>)}
              </tr>)}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function rate(value: number | null, part: number, total: number): string {
  return value === null ? "?" : `${value.toFixed(1)}% (${part}/${total})`;
}

export function Quality({ live }: { live: boolean }) {
  const { data, error } = useQuality(live);
  const empty = !live ? "Offline" : data ? "No observations in this window." : error ? "Unavailable" : "Loading…";
  return (
    <>
      {error && <p role="alert" className="mb-3 text-label-13 text-red-700">{error}</p>}
      <Figures title="First-Pass Land Rate" population="Verified and landed runs · Last 30 days" empty={empty}
        columns={["Project", "Builder model", "Effort", "Verified", "First pass", "Landed", "Median rounds"]}
        rows={data?.builders.map(({ group: g, counts: c }) =>
          [g.project, g.model, g.effort, c.verified, rate(c.first_pass_percent, c.first_pass, c.verified), c.landed, c.median_rounds ?? "?"]) ?? []} />
      <Figures title="Finding Precision" population="Findings raised · Last 30 days" empty={empty}
        columns={["Project", "Role", "Reviewer model", "Effort", "Lens", "Findings", "Fixed", "Declined", "Stalemate"]}
        rows={data?.findings.map(({ group: g, lens, counts: c }) =>
          [g.project, g.role, g.model, g.effort, lens, c.findings, rate(c.fixed_percent, c.fixed, c.fixed + c.declined),
            rate(c.declined_percent, c.declined, c.fixed + c.declined), rate(c.stalemate_percent, c.stalled, c.covered)]) ?? []} />
      <Figures title="Spend per Landed Card" population="Cards landed · Last 30 days" empty={empty}
        columns={["Project", "Role", "Model", "Effort", "Cards", "Average spend"]}
        rows={data?.spend.map(({ group: g, spend: s }) => [g.project, g.role, g.model, g.effort, s.cards,
          dollars(Math.round(s.average_dollars * 100)) + (s.unknown ? " + ?" : "")]) ?? []} />
      <Figures title="Tool Failures" population="Measured card turns · Last 30 days" empty={empty}
        columns={["Project", "Role", "Model", "Effort", "Turns", "Calls", "Failed", "Failure rate"]}
        rows={data?.tools.map(({ group: g, counts: c }) =>
          [g.project, g.role, g.model, g.effort, c.turns, c.calls, c.failed, rate(c.percent, c.failed, c.calls)]) ?? []} />
    </>
  );
}
