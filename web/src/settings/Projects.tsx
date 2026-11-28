import { useEffect } from "react";
import { replaceRoute, settingsProjectPath } from "../route";
import { type Editor, useConfig, useConfigSection } from "../config";
import { AutoDelegation } from "../Project";
import { RefusalLine, useHandOffers } from "../offers";
import type { ConfigSection, Screen } from "../types";
import { Row } from "../ui/List";
import { Segmented } from "../ui/Segmented";
import { SettingsPage } from "./Page";
import { type Config, ConfigState, locked } from "./ConfigTabs";
import { Entries, Field, Group, Labelled, SaveBar, Setting, Withheld } from "./Fields";

const description = "Save writes these to mork.toml and applies them to the running engine. Auto delegation changes at once.";

const passes = [
  ["pulse", "Pulse", "Reads what landed and the cards that ended since its last run."],
  ["deep-sweep", "Deep sweep", "Reads the whole code for dead code, dead tests, risks and wording."],
  ["dream", "Dream", "Consolidates this project's memory and folds its lesson notes."],
] as const;
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

function Switch({ editor, name, label, options }: { editor: Editor; name: string; label: string; options: readonly string[] }) {
  const v = editor.value(name);
  if (!v || !("text" in v)) return <Withheld />;
  return <Segmented compact options={options} value={v.text} label={cap} onChange={text => editor.edit(name, { text }, label)} />;
}

function Pass({ editor, read, title, words }: { editor: Editor; read: string; title: string; words: string }) {
  const prefix = "passes." + read + ".";
  const v = editor.value(prefix + "mode");
  const budget = editor.fields.get(prefix + "budget")?.default;
  return <div>
    <Setting title={title} words={words}><Switch editor={editor} name={prefix + "mode"} label={title} options={["manual", "scheduled", "auto"]} /></Setting>
    {v && "text" in v && v.text === "scheduled" && <div className="border-t border-gray-400"><Row indent={1}>
      <span className="w-[176px] shrink-0 text-label-12 text-gray-700">Starts when one is reached</span>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {["landings", "edits", "notes", "days"].map(k => <label key={k} className="flex items-center gap-2 text-label-12 text-gray-900">
          {cap(k)}<Field editor={editor} name={prefix + k} label={title + " " + k} placeholder="off" /></label>)}
      </div></Row></div>}
    {read === "dream" && <div className="border-t border-gray-400"><Row indent={1}>
      <span className="w-[176px] shrink-0 text-label-12 text-gray-700">Memory files kept</span>
      <Field editor={editor} name={prefix + "budget"} label="Dream memory files kept" placeholder={budget && "text" in budget ? budget.text : undefined} />
    </Row></div>}
  </div>;
}

function ProjectPage({ section, config, auto, live }: { section: ConfigSection; config: Config; auto: boolean | undefined; live: boolean }) {
  const editor = useConfigSection(section, config.refresh);
  const project = section.project!;
  const autoOffers = useHandOffers();
  // The heading sits outside the body's fieldset, so its Save and Discard take the same lock.
  const readOnly = locked(config, live);
  return (
    <SettingsPage crumb="Projects" title={project} words={description}
      right={editor.dirty ? <SaveBar editor={editor} locked={readOnly} /> : undefined}>
      <fieldset className="min-w-0" disabled={readOnly || editor.busy}>
        <ConfigState config={config} live={live} />
        <RefusalLine refusal={editor.refusal || null} className="mb-4" />
        <Group title="Cards">
          <Setting title="Auto delegation" words="Each card that starts in this project starts delegated. Changes at once, without Save.">
            <AutoDelegation project={project} auto={auto} live={live} offers={autoOffers} compact />
          </Setting>
          <RefusalLine refusal={autoOffers.refusals[project] ?? null} />
          <Setting title="Colleague review" words="Standard keeps the lighter rules for colleagues' work. Full reviews it as our own.">
            <Switch editor={editor} name="colleague_review" label="Colleague review" options={["standard", "full"]} /></Setting>
          <Setting title="Test slots" words="Test and check runs this project may hold at one time.">
            <Field editor={editor} name="slots" label="Test slots" /></Setting>
        </Group>
        <Group title="Checks" note="run on each submitted head, before review · leave both empty for none">
          {["command", "folder"].map(k => <Labelled key={k} label={cap(k)}><Field editor={editor} name={"checks." + k} label={"Checks " + k} width="fill" /></Labelled>)}
        </Group>
        <Group title="Look back">
          {passes.map(([read, title, words]) => <Pass key={read} editor={editor} read={read} title={title} words={words} />)}
        </Group>
        <Group title="Wall reach" note="besides the global lists in System">
          <Entries editor={editor} name="domains" title="Domains" words="Hosts this project's walls reach." placeholder="registry.example or registry.example:8443" />
          <Entries editor={editor} name="ports" title="Ports" words="More outbound ports for this project's install and build jobs." placeholder="8443 or 8000-8010" />
          <Entries editor={editor} name="host_ports" title="Host ports" words="Loopback ports on this machine its walls reach. 1024 and up." placeholder="3306 or 6300-6302" />
        </Group>
      </fieldset>
    </SettingsPage>
  );
}

export function Projects({ project, screen, live }: { project?: string; screen: Screen; live: boolean }) {
  const config = useConfig(live);
  const sections = config.view?.sections;
  const first = sections?.find(s => s.section === "project")?.project;
  useEffect(() => {
    if (project === undefined && first) replaceRoute(settingsProjectPath(first));
  }, [project, first]);

  const section = sections?.find(s => s.section === "project" && s.project === project);
  if (section) return <ProjectPage key={project} section={section} config={config} live={live}
    auto={screen.boards.find(b => b.project === project)?.auto} />;
  return <SettingsPage crumb="Projects" title={project ?? "Projects"} words={description}>
    <ConfigState config={config} live={live} />
    {config.view && project !== undefined && <p className="text-label-13 text-gray-700">No project {project} in mork.toml.</p>}
  </SettingsPage>;
}
