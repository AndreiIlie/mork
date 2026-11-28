import { useState, type ReactNode } from "react";
import type { Editor } from "../config";
import { ChevronIcon } from "../icons";
import { EffortRow, VENDOR, VendorRow } from "../SeatModel";
import type { ConfigField, ModelChoice, PickModel, SeatDefault } from "../types";
import { Button } from "../ui/Button";
import { List, Row } from "../ui/List";
import { TextLink } from "../ui/TextLink";
import { Section, Withheld } from "./Fields";

const seatNames: Record<string, string> = { planner: "Planner", foreman: "Foreman", "review-foreman": "Review foreman", reviewer: "Reviewer", researcher: "Researcher", baseline: "Baseline" };
const standings: Record<string, string> = { work: "build", report: "report", inside: "our work", outside: "outside work" };
const listNames: Record<string, [string, string]> = {
  researcher: ["Researchers", "What a planner or foreman may give a researcher."],
  "reviewer.inside": ["Reviewers, our work", "What a review foreman may give a reviewer of our own cards."],
  "reviewer.outside": ["Reviewers, outside work", "The same, for colleagues' pull requests and branches."],
  pass: ["Auto passes", "The model and effort pairs an auto look back chooses from."],
};

function sameModel(a: Pick<ModelChoice, "cli" | "model">, b: Pick<ModelChoice, "cli" | "model">) {
  return a.cli === b.cli && a.model === b.model;
}

function Panel({ children, top = true }: { children: ReactNode; top?: boolean }) {
  return <div className={(top ? "border-t border-gray-400 " : "") + "bg-background-200 py-3 pl-[calc(var(--spacing-edge)+24px)] pr-edge"}>
    <div className="flex flex-col gap-2">{children}</div></div>;
}

function Models({ choices, lit, select }: {
  choices: PickModel[]; lit: SeatDefault | null; select: (pick: PickModel) => void;
}) {
  return <>{[...new Set(choices.map(c => c.cli))].map(cli =>
    <VendorRow key={cli} cli={cli} choices={choices.filter(c => c.cli === cli)} lit={lit} fresh={false} disabled={false}
      onPick={c => { const model = choices.find(m => sameModel(m, c)); if (model) select(model); }} />)}</>;
}

function SeatRow({ editor, field, open, toggle }: { editor: Editor; field: ConfigField; open: boolean; toggle: () => void }) {
  const [name, part] = field.key.split(".");
  const standing = ["foreman", "review-foreman", "reviewer"].includes(name) ? standings[part] : "";
  const v = editor.value(field.key);
  const seat = v && "seat" in v ? v.seat : null;
  const offered = seat && field.offers.find(m => sameModel(m, seat));
  const fallback = field.default && "seat" in field.default ? field.default.seat : null;
  const label = standing ? seatNames[name] + " · " + standing : seatNames[name];
  const edit = (next: SeatDefault) => editor.edit(field.key, { seat: next }, label);
  function pick(cli: SeatDefault["cli"], model: string, effort: SeatDefault["effort"]) {
    const saved = field.value && "seat" in field.value ? field.value.seat : null;
    for (const existing of [seat, saved, fallback]) {
      if (existing && existing.cli === cli && existing.model === model && existing.effort === effort) {
        edit(existing);
        return;
      }
    }
    edit({ cli, model, effort, configured: true });
  }
  return <div>
    <Row onClick={toggle} expanded={open}>
      <ChevronIcon open={open} className="text-gray-900" />
      <span className="flex min-w-0 flex-1 items-baseline gap-2">
        <span className="text-label-14 text-gray-1000">{seatNames[name]}</span>
        <span className="text-label-12 text-gray-700">{standing}</span>
      </span>
      {seat ? <>
        <span className="w-[56px] shrink-0 text-label-12 text-gray-700">{seat.configured ? "" : "default"}</span>
        <span className="w-[56px] shrink-0 text-label-12 text-gray-700">{VENDOR[seat.cli] ?? seat.cli}</span>
        <span className="w-[112px] shrink-0 truncate text-label-13 text-gray-1000">{offered?.label ?? seat.model}</span>
        <span className="w-[48px] shrink-0 text-right font-mono text-label-12 text-gray-900">{seat.effort}</span>
      </> : <Withheld />}
    </Row>
    {open && seat && <Panel>
      <Models choices={field.offers} lit={seat} select={m => pick(m.cli, m.model, m.efforts.includes(seat.effort) ? seat.effort : "high")} />
      <EffortRow efforts={offered?.efforts ?? []} selected={[seat.effort]} onPick={effort => pick(seat.cli, seat.model, effort)} />
      <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-2">
        <p className="text-label-12 text-gray-900">Used when a seat first starts, and when its configured choice expires. A picked model stays.</p>
        {seat.configured && fallback && <TextLink onClick={() => edit(fallback)}>Use default</TextLink>}
      </div>
    </Panel>}
  </div>;
}

function PickRows({ editor, field, open, toggle }: { editor: Editor; field: ConfigField; open: boolean; toggle: () => void }) {
  const v = editor.value(field.key);
  const picks = v && "picks" in v ? v.picks : null;
  const [title, words] = listNames[field.key];
  const edit = (picks: PickModel[]) => editor.edit(field.key, { picks }, title);
  const remaining = field.offers.filter(m => !picks?.some(p => sameModel(m, p)));
  return <div className="mb-4">
    <div className="mb-2 flex min-h-control flex-wrap items-center gap-x-3 gap-y-1">
      <h3 className="text-label-14 text-gray-1000">{title}</h3>
      <span className="text-label-12 text-gray-700">{words}</span>
      <span className="ml-auto">{picks ? <Button tier="secondary" size="sm" pressed={open}
        disabled={!remaining.length} onClick={toggle}>Add model</Button> : <Withheld />}</span>
    </div>
    {picks && <List>
      {open && <Panel top={false}><Models choices={remaining} lit={null} select={m => {
        edit([...picks, { ...m, efforts: m.efforts.includes("high") ? ["high"] : m.efforts.slice(0, 1) }]); toggle();
      }} /></Panel>}
      {!picks.length && <Row><span className="text-label-12 text-gray-700">None listed: nothing may be chosen here.</span></Row>}
      {picks.map((pick, index) => {
        const offered = field.offers.find(m => sameModel(m, pick))?.efforts ?? [];
        const efforts = [...new Set([...offered, ...pick.efforts])];
        return <Row key={index} actions={<>
          <EffortRow caption={false} efforts={efforts} selected={pick.efforts} onPick={e => edit(picks.map((p, i) =>
            i === index ? { ...p, efforts: p.efforts.includes(e) ? p.efforts.filter(x => x !== e) : [...p.efforts, e] } : p))} />
          <Button tier="ghost" size="sm" onClick={() => edit(picks.filter((_, i) => i !== index))}>Remove</Button>
        </>}>
          <span className="w-[56px] shrink-0 text-label-12 text-gray-700">{VENDOR[pick.cli] ?? pick.cli}</span>
          <span className="text-label-14 text-gray-1000">{pick.label}</span>
        </Row>;
      })}
    </List>}
  </div>;
}

export function SeatSettings({ editor }: { editor: Editor }) {
  const [open, setOpen] = useState("");
  const seats = editor.section.section === "seats";
  const Component = seats ? SeatRow : PickRows;
  const rows = editor.section.fields.map(field => <Component key={field.key} editor={editor} field={field}
    open={open === field.key} toggle={() => setOpen(open === field.key ? "" : field.key)} />);
  return <Section editor={editor} title={seats ? "Seat defaults" : "Model picks"}
    note={seats ? "used when a seat first starts" : "what may be chosen for a child seat, and at which efforts"}>
    {seats ? <List>{rows}</List> : rows}
  </Section>;
}
