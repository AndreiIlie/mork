import { useConfig, useConfigSection, type RefreshConfig } from "../config";
import type { ConfigSection, Screen } from "../types";
import { ProjectSettings } from "./Projects";
import { SeatSettings } from "./Seats";
import { SystemSettings } from "./System";

function Group({ section, refresh, screen, live }: { section: ConfigSection; refresh: RefreshConfig; screen: Screen; live: boolean }) {
  const editor = useConfigSection(section, refresh);
  if (section.section === "project") return <ProjectSettings editor={editor} live={live} auto={screen.boards.find(b => b.project === section.project)?.auto} />;
  if (section.section === "seats" || section.section === "picks") return <SeatSettings editor={editor} />;
  return <SystemSettings editor={editor} />;
}

const groups = { projects: ["project"], seats: ["seats", "picks"], system: ["limits", "slice", "reach"] };

export function ConfigTabs({ tab, screen, live }: { tab: keyof typeof groups; screen: Screen; live: boolean }) {
  const { view, error, refresh } = useConfig(live);
  return <fieldset className="min-w-0" disabled={!live || !view || view.stale || !!error}>
    {error && <p role="alert" className="mb-4 text-copy-13 text-red-900">{error}</p>}
    {view?.stale && <p role="alert" className="mb-4 text-copy-13 text-red-900">mork.toml changed on disk since Mork started. Reload Mork to edit it here.</p>}
    {!view && !error && <p className="text-label-13 text-gray-700">{live ? "Loading settings…" : "Offline"}</p>}
    {view?.sections.filter(s => groups[tab].includes(s.section)).map(s =>
      <Group key={s.section + "/" + (s.project ?? "")} section={s} refresh={refresh} screen={screen} live={live} />)}
  </fieldset>;
}
