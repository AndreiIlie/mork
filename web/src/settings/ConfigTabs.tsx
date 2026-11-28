import { SeatSettings } from "./Seats";
import { SystemSettings } from "./System";
import { type RefreshConfig, useConfig, useConfigSection } from "../config";
import type { ConfigSection } from "../types";

export type Config = ReturnType<typeof useConfig>;

export function ConfigState({ config, live }: { config: Config; live: boolean }) {
  const { view, error } = config;
  return <>
    {error && <p role="alert" className="mb-4 text-copy-13 text-red-900">{error}</p>}
    {view?.stale && <p role="alert" className="mb-4 text-copy-13 text-red-900">mork.toml changed on disk since Mork started. Reload Mork to edit it here.</p>}
    {!view && !error && <p className="text-label-13 text-gray-700">{live ? "Loading settings…" : "Offline"}</p>}
  </>;
}

export function locked(config: Config, live: boolean): boolean {
  return !live || !config.view || config.view.stale || !!config.error;
}

function Group({ section, refresh }: { section: ConfigSection; refresh: RefreshConfig }) {
  const editor = useConfigSection(section, refresh);
  if (section.section === "seats" || section.section === "picks") return <SeatSettings editor={editor} />;
  return <SystemSettings editor={editor} />;
}

const groups = { seats: ["seats", "picks"], system: ["limits", "slice", "reach"] };

export function ConfigTabs({ tab, live }: { tab: keyof typeof groups; live: boolean }) {
  const config = useConfig(live);
  return <fieldset className="min-w-0" disabled={locked(config, live)}>
    <ConfigState config={config} live={live} />
    {config.view?.sections.filter(s => groups[tab].includes(s.section)).map(s =>
      <Group key={s.section} section={s} refresh={config.refresh} />)}
  </fieldset>;
}
