import type { Editor } from "../config";
import { List } from "../ui/List";
import { Entries, Field, Section, Setting } from "./Fields";

const limits = [
  ["max_test_runs", "Test runs at once", "Test and check runs across every project."],
  ["context_cap", "Context cap", "Claude and agy sessions compact at this many tokens. 0 turns the cap off."],
  ["fresh_context_percent", "Fresh session at", "A foreman taking a fix or a join starts fresh past this percent of its context. 0 turns it off."],
] as const;
const resources = [
  ["cpu_weight", "CPU weight", "The jobs' share of the CPU against the desktop's 100. 1 to 10000."],
  ["io_weight", "IO weight", "The jobs' share of disk IO against the desktop's 100. 1 to 10000."],
  ["memory_high", "Memory high", "Jobs are slowed past this: a share of the RAM (75%) or a size (24G)."],
] as const;

export function SystemSettings({ editor }: { editor: Editor }) {
  if (editor.section.section === "reach") return <Section title="Wall reach" note="every project's walls" editor={editor}>
    <List>
      <Entries editor={editor} name="domains" title="Domains" words="Hosts every walled turn reaches through the proxy." placeholder="api.example.com or api.example.com:8443" />
      <Entries editor={editor} name="ports" title="Ports" words="Outbound ports for install, build and probe jobs." placeholder="8443" />
    </List>
  </Section>;
  const slice = editor.section.section === "slice";
  return <Section title={slice ? "Job resources" : "Limits"} note={slice ? "applies at the next reload" : undefined} editor={editor}>
    <List>{(slice ? resources : limits).map(([key, title, words]) => <Setting key={key} title={title} words={words}>
      <Field editor={editor} name={key} label={title} width={key === "context_cap" ? "md" : "sm"} />
    </Setting>)}</List>
  </Section>;
}
