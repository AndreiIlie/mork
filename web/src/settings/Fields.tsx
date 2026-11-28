import { useState, type ReactNode } from "react";
import type { Editor } from "../config";
import { ChevronIcon } from "../icons";
import { RefusalLine } from "../offers";
import { Button } from "../ui/Button";
import { List, Row } from "../ui/List";
import { SectionHead } from "../ui/Section";
import { TextInput } from "../ui/TextInput";

export function Group({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return <section className="mb-8 min-w-0">
    <SectionHead title={title} note={note} />
    <List>{children}</List>
  </section>;
}

export function SaveBar({ editor, locked = false }: { editor: Editor; locked?: boolean }) {
  return <>
    <span className="text-label-12 text-gray-700">Unsaved changes</span>
    <Button tier="ghost" size="sm" disabled={editor.busy || locked} onClick={editor.discard}>Discard</Button>
    <Button tier="primary" size="sm" disabled={editor.busy || locked} onClick={editor.save}>Save</Button>
  </>;
}

export function Section({ title, note, editor, children }: { title: string; note?: string; editor: Editor; children: ReactNode }) {
  return <fieldset disabled={editor.busy} className="mb-8 min-w-0">
    <SectionHead title={title} note={note} right={editor.dirty ? <SaveBar editor={editor} /> : undefined} />
    <RefusalLine refusal={editor.refusal || null} className="mb-2" />
    {children}
  </fieldset>;
}

function SettingText({ title, words }: { title: string; words: string }) {
  return <div className="min-w-0 flex-1">
    <div className="text-label-14 text-gray-1000">{title}</div>
    <div className="mt-0.5 text-label-12 text-gray-700">{words}</div>
  </div>;
}

export function Labelled({ label, children }: { label: string; children: ReactNode }) {
  return <Row><span className="w-[144px] shrink-0 text-label-14 text-gray-1000">{label}</span>{children}</Row>;
}

export function Setting({ title, words, children }: { title: string; words: string; children?: ReactNode }) {
  return <Row actions={children}><SettingText title={title} words={words} /></Row>;
}

export function Withheld() {
  return <span className="text-label-12 text-gray-700">Withheld: it holds a secret Mork knows. Edit it in mork.toml.</span>;
}

export function Field({ editor, name, label, width = "sm", placeholder }: {
  editor: Editor; name: string; label: string; width?: "sm" | "md" | "fill"; placeholder?: string;
}) {
  const v = editor.value(name);
  if (!v || !("text" in v)) return <Withheld />;
  return <TextInput label={label} width={width} value={v.text} placeholder={placeholder}
    onChange={e => editor.edit(name, { text: e.target.value }, label)} />;
}

export function Entries({ editor, name, title, words, placeholder }: {
  editor: Editor; name: string; title: string; words: string; placeholder: string;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const v = editor.value(name);
  if (!v || !("entries" in v)) return <Setting title={title} words={words}><Withheld /></Setting>;
  const entries = v.entries;
  const add = () => {
    const entry = draft.trim();
    if (!entry || entries.includes(entry)) return;
    editor.edit(name, { entries: [...entries, entry] }, title);
    setDraft("");
  };
  return <div>
    <Row expanded={open} onClick={() => setOpen(!open)}>
      <ChevronIcon open={open} className="text-gray-900" />
      <SettingText title={title} words={words} />
      <span className="ml-auto font-mono text-label-12 tabular-nums text-gray-900">{entries.length}</span>
    </Row>
    {open && <div className="divide-y divide-gray-400 border-t border-gray-400">
      {entries.map((entry, i) => <Row key={i} indent={1} actions={<Button tier="ghost" size="sm"
        onClick={() => editor.edit(name, { entries: entries.filter((_, index) => index !== i) }, title)}>Remove</Button>}>
        <span className="font-mono text-label-13 text-gray-1000">{entry}</span></Row>)}
      <Row indent={1} actions={<Button tier="secondary" size="sm" disabled={!draft.trim() || entries.includes(draft.trim())} onClick={add}>Add</Button>}>
        <TextInput width="fill" label={"Add to " + title} value={draft} onChange={e => setDraft(e.target.value)} placeholder={placeholder} />
      </Row>
    </div>}
  </div>;
}
