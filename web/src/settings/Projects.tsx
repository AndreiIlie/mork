import type { Editor } from "../config";
import { RefusalLine, useHandOffers } from "../offers";
import { Row } from "../ui/List";
import { Segmented } from "../ui/Segmented";
import { Entries, Field, Section, Setting, Withheld } from "./Fields";

const passes = [
  ["pulse", "Pulse", "Reads what landed and the cards that ended since its last run."],
  ["deep-sweep", "Deep sweep", "Reads the whole code for dead code, dead tests, risks and wording."],
  ["dream", "Dream", "Consolidates this project's memory and folds its lesson notes."],
] as const;
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

function Switch({ editor, name, options }: { editor: Editor; name: string; options: readonly string[] }) {
  const v = editor.value(name);
  if (!v || !("text" in v)) return <Withheld />;
  function select(text: string) {
    editor.edit(name, { text });
    if (text === "auto") {
      const prefix = name.slice(0, -4);
      for (const key of ["landings", "edits", "notes", "days"]) editor.edit(prefix + key, { text: "" });
    }
  }
  return <Segmented compact options={options} value={v.text} label={cap} onChange={select} />;
}

function Pass({ editor, read, title, words }: { editor: Editor; read: string; title: string; words: string }) {
  const prefix = "passes." + read + ".";
  const v = editor.value(prefix + "mode");
  return <div>
    <Setting title={title} words={words}><Switch editor={editor} name={prefix + "mode"} options={["manual", "scheduled", "auto"]} /></Setting>
    {v && "text" in v && v.text === "scheduled" && <div className="border-t border-gray-400"><Row indent={1}>
      <span className="w-[176px] shrink-0 text-label-12 text-gray-700">Starts when one is reached</span>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {["landings", "edits", "notes", "days"].map(k => <label key={k} className="flex items-center gap-2 text-label-12 text-gray-900">
          {cap(k)}<Field editor={editor} name={prefix + k} label={title + " " + k} placeholder="off" /></label>)}
      </div></Row></div>}
    {read === "dream" && <div className="border-t border-gray-400"><Row indent={1}>
      <span className="w-[176px] shrink-0 text-label-12 text-gray-700">Memory files kept</span>
      <Field editor={editor} name={prefix + "budget"} label="Dream budget" placeholder="60" />
    </Row></div>}
  </div>;
}

export function ProjectSettings({ editor, auto }: { editor: Editor; auto: boolean | undefined }) {
  const project = editor.section.project!;
  const { pending, refusals, press } = useHandOffers();
  return <Section title={project} editor={editor}>
    <Setting title="Auto delegation" words="Each card that starts in this project starts delegated. Changes at once.">
      <Segmented compact options={["Off", "On"] as const} value={auto ? "On" : "Off"} disabled={() => auto === undefined || !!pending[project]}
        onChange={v => { if ((v === "On") !== auto) press(project, { label: "Auto delegation", command: { auto: { project, off: v === "Off" } } }); }} />
    </Setting>
    <RefusalLine refusal={refusals[project] ?? null} />
    <Setting title="Colleague review" words="Standard keeps the lighter rules for colleagues' work. Full reviews it as our own.">
      <Switch editor={editor} name="colleague_review" options={["standard", "full"]} /></Setting>
    <Setting title="Test slots" words="Test and check runs this project may hold at one time.">
      <Field editor={editor} name="slots" label="Test slots" /></Setting>
    <div><Setting title="Checks" words="Run on each submitted head, before review. Leave both empty for none." />
      <div className="divide-y divide-gray-400 border-t border-gray-400">
        {["command", "folder"].map(k => <Row key={k} indent={1}><span className="w-[176px] shrink-0 text-label-12 text-gray-700">{cap(k)}</span>
          <Field editor={editor} name={"checks." + k} label={"Checks " + k} width="fill" /></Row>)}
      </div></div>
    {passes.map(([read, title, words]) => <Pass key={read} editor={editor} read={read} title={title} words={words} />)}
    <Entries editor={editor} name="domains" title="Domains" words="Hosts this project's walls reach, besides the global list." placeholder="registry.example or registry.example:8443" />
    <Entries editor={editor} name="ports" title="Ports" words="More outbound ports for this project's install and build jobs." placeholder="8443 or 8000-8010" />
    <Entries editor={editor} name="host_ports" title="Host ports" words="Loopback ports on this machine its walls reach. 1024 and up." placeholder="3306 or 6300-6302" />
  </Section>;
}
