import { type ReactNode, useState } from "react";
import { dollars } from "./format";
import { useQuality } from "./quality";
import type { Quality as Figures } from "./types";
import { KpiStrip } from "./ui/Kpi";
import { ProgressBar } from "./ui/ProgressBar";
import { SectionHead } from "./ui/Section";
import { Segmented } from "./ui/Segmented";
import { List, Row, type TableColumn } from "./ui/List";
import { SettingsPage } from "./settings/Page";

const whole = new Intl.NumberFormat("en-US");

function percent(part: number, total: number): string {
  return total === 0 ? "?" : ((part * 100) / total).toFixed(1) + "%";
}

function Rate({ value, part, total }: { value: number | null; part: number; total: number }) {
  return <span className="font-mono tabular-nums">
    <span className="text-gray-1000">{value === null ? "?" : value.toFixed(1) + "%"}</span>
    <span className="ml-2 inline-block min-w-[64px] text-left text-gray-700">{whole.format(part)}/{whole.format(total)}</span>
  </span>;
}

function SeatIdentity({ role, model, effort }: { role?: string; model: string; effort: string }) {
  return <span className="flex items-baseline gap-2">
    {role !== undefined && <span className="w-[112px] shrink-0 text-gray-900">{role}</span>}
    <span className="text-gray-1000">{model}</span>
    <span className="font-mono text-label-12 text-gray-700">{effort}</span>
  </span>;
}

function numericColumns(...labels: string[]): TableColumn[] {
  return labels.map(label => ({ label, kind: "number" }));
}

function Table({ title, note, columns, rows, empty }: {
  title: string;
  note: string;
  columns: TableColumn[];
  rows: ReactNode[][];
  empty: string;
}) {
  return (
    <section className="mb-8">
      <SectionHead title={title} note={note} />
      {rows.length === 0 ? <p className="text-label-13 text-gray-700">{empty}</p> : (
        <List table={{ label: title, columns }}>
          {rows.map((cells, i) => <Row key={i} columns={columns} cells={cells} />)}
        </List>
      )}
    </section>
  );
}

function summary(builders: Figures["builders"], findings: Figures["findings"], tools: Figures["tools"]) {
  const sum = (values: number[]) => values.reduce((a, b) => a + b, 0);
  const verified = sum(builders.map(r => r.counts.verified));
  const first = sum(builders.map(r => r.counts.first_pass));
  const fixed = sum(findings.map(r => r.counts.fixed));
  const declined = sum(findings.map(r => r.counts.declined));
  const calls = sum(tools.map(r => r.counts.calls));
  const failed = sum(tools.map(r => r.counts.failed));
  return [
    { label: "First Pass", value: percent(first, verified), sub: `${first} of ${verified} verified runs` },
    { label: "Findings Fixed", value: percent(fixed, fixed + declined), sub: `${fixed} of ${fixed + declined} decided findings` },
    { label: "Tool Failures", value: percent(failed, calls), sub: `${whole.format(failed)} of ${whole.format(calls)} tool calls` },
  ];
}

export function Quality({ live }: { live: boolean }) {
  const { data, error } = useQuality(live);
  const projects = data ? [...new Set([...data.builders, ...data.findings, ...data.spend, ...data.tools].map(row => row.group.project))] : [];
  const [picked, setPicked] = useState<string | null>(null);
  const project = picked !== null && projects.includes(picked) ? picked : projects[0] ?? "";
  const empty = !live ? "Offline" : data ? "No observations in this window." : error ? "Unavailable" : "Loading…";
  const inProject = (row: { group: { project: string } }) => row.group.project === project;
  const builders = data?.builders.filter(inProject) ?? [];
  const findings = data?.findings.filter(inProject) ?? [];
  const spend = data?.spend.filter(inProject) ?? [];
  const tools = data?.tools.filter(inProject) ?? [];
  return (
    <SettingsPage title="Quality" words="How builds and reviews went over the last 30 days, one project at a time."
      right={projects.length === 0 ? undefined : <>
        <span id="quality-project" className="text-label-12 text-gray-700">Project</span>
        {projects.length === 1 ? <span className="text-label-13 text-gray-1000">{project}</span> : (
          <div role="group" aria-labelledby="quality-project" className="max-w-[480px] overflow-x-auto">
            <Segmented options={projects} value={project} onChange={setPicked} />
          </div>
        )}
      </>}>
      {error && <p role="alert" className="mb-3 text-label-13 text-red-700">{error}</p>}
      {data && <div className="mb-8"><KpiStrip items={summary(builders, findings, tools)} /></div>}
      <Table title="First-Pass Land Rate" note="verified and landed runs" empty={empty}
        columns={[{ label: "Builder", kind: "identity" }, { label: "First pass" }, ...numericColumns("Verified", "Landed", "Median rounds")]}
        rows={builders.map(({ group: g, counts: c }) => [
          <SeatIdentity model={g.model} effort={g.effort} />,
          <span className="flex items-center gap-3">
            <span className="w-[120px] shrink-0"><ProgressBar percent={c.first_pass_percent ?? 0} tone="neutral" /></span>
            <Rate value={c.first_pass_percent} part={c.first_pass} total={c.verified} />
          </span>,
          c.verified, c.landed, c.median_rounds ?? "?"])} />
      <Table title="Finding Precision" note="findings raised" empty={empty}
        columns={[{ label: "Reviewer", kind: "identity" }, { label: "Lens" }, ...numericColumns("Findings", "Fixed", "Declined", "Stalemate")]}
        rows={findings.map(({ group: g, lens, counts: c }, i) => {
          const before = findings[i - 1]?.group;
          const repeated = before !== undefined && before.role === g.role && before.model === g.model && before.effort === g.effort;
          return [
            repeated ? "" : <SeatIdentity role={g.role} model={g.model} effort={g.effort} />,
            <span className="text-gray-1000">{lens}</span>,
            c.findings,
            <Rate value={c.fixed_percent} part={c.fixed} total={c.fixed + c.declined} />,
            <Rate value={c.declined_percent} part={c.declined} total={c.fixed + c.declined} />,
            <Rate value={c.stalemate_percent} part={c.stalled} total={c.covered} />];
        })} />
      <Table title="Spend per Landed Card" note="cards landed" empty={empty}
        columns={[{ label: "Seat", kind: "identity" }, ...numericColumns("Cards", "Average spend")]}
        rows={spend.map(({ group: g, spend: s }) => [
          <SeatIdentity role={g.role} model={g.model} effort={g.effort} />, s.cards,
          <span className="text-gray-1000">{dollars(Math.round(s.average_dollars * 100)) + (s.unknown ? " + ?" : "")}</span>])} />
      <Table title="Tool Failures" note="measured card turns" empty={empty}
        columns={[{ label: "Seat", kind: "identity" }, ...numericColumns("Turns", "Calls", "Failed", "Failure rate")]}
        rows={tools.map(({ group: g, counts: c }) => [
          <SeatIdentity role={g.role} model={g.model} effort={g.effort} />, whole.format(c.turns), whole.format(c.calls), whole.format(c.failed),
          <span className="text-gray-1000">{c.percent === null ? "?" : c.percent.toFixed(1) + "%"}</span>])} />
    </SettingsPage>
  );
}
