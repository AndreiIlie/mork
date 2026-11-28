import { useEffect } from "react";
import { Setting } from "./settings/Fields";
import { ConfigTabs } from "./settings/ConfigTabs";
import { ReloadChangelog } from "./ReloadChangelog";
import { Usage } from "./Usage";
import { Quality } from "./Quality";
import type { AlertPermission } from "./alerts";
import type { Reload } from "./reload";
import { RefusalLine } from "./offers";
import { SETTINGS_PATH, type SettingsTab, navigate } from "./route";
import type { PlaceView, RecentBurn, Screen } from "./types";
import { Button } from "./ui/Button";
import { List, Row } from "./ui/List";
import { PageHeader, PageScroll } from "./ui/Page";
import { Segmented } from "./ui/Segmented";

const TABS = ["general", "projects", "seats", "system", "usage", "quality", "updates"] as const;
const TAB_LABEL: Record<SettingsTab, string> = { general: "General", projects: "Projects", seats: "Seats", system: "System", usage: "Usage", quality: "Quality", updates: "Updates" };
const ALERT_WORDS = { granted: "Enabled", denied: "Blocked in this browser's site settings", unsupported: "Unavailable in this browser" } as const;

function General({ alerts, resourcesOn, setResourcesOn }: { alerts: AlertPermission; resourcesOn: boolean; setResourcesOn: (on: boolean) => void }) {
  const { permission, request } = alerts;
  return (
    <List>
      <Setting title="Alerts" words="A browser notification when a hand is raised while this tab is in the background.">
        {permission === "default" ? (
          <Button tier="secondary" size="sm" onClick={request}>Enable alerts</Button>
        ) : (
          <span className="text-label-12 text-gray-900">{ALERT_WORDS[permission]}</span>
        )}
      </Setting>
      <Setting title="Resource tracker" words="CPU and RAM in the sidebar, above the quotas.">
        <Segmented compact options={["Off", "On"] as const} value={resourcesOn ? "On" : "Off"} onChange={(v) => setResourcesOn(v === "On")} />
      </Setting>
    </List>
  );
}

function Updates({ screen, build, view, live, reload }: { screen: Screen; build: string | null; view?: PlaceView; live: boolean; reload: Reload }) {
  const pin = screen.top.pin;
  return (
    <>
      <List>
        <Row actions={
          <Button tier={pin.ready ? "primary" : "secondary"} size="sm" title="Reload Mork to its pinned build"
            disabled={!live || reload.busy} onClick={() => reload.press("updates")}>
            Reload
          </Button>
        }>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-label-14 text-gray-1000">Build</span>
              <span className="font-mono text-label-13 text-gray-1000">{build?.slice(0, 8) ?? "offline"}</span>
            </div>
            {pin.ready && (
              <div className="mt-0.5 text-label-12 text-gray-700">Update ready <span className="font-mono text-gray-1000">{pin.pin}</span></div>
            )}
          </div>
        </Row>
      </List>
      <RefusalLine refusal={reload.refusal("updates")} className="mt-2" />
      <ReloadChangelog view={view} live={live} tzOffsetMin={screen.tz_offset_min} />
    </>
  );
}

export function Settings({ reload, tab, returnPath, screen, burn, burnError, build, mainView, nowMs, live, alerts, resourcesOn, setResourcesOn }: {
  reload: Reload;
  tab: SettingsTab;
  returnPath: string;
  screen: Screen;
  burn: RecentBurn | null;
  burnError: string | null;
  build: string | null;
  mainView?: PlaceView;
  nowMs: number;
  live: boolean;
  alerts: AlertPermission;
  resourcesOn: boolean;
  setResourcesOn: (on: boolean) => void;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      const target = event.target;
      if (target instanceof HTMLElement && (target.isContentEditable || target.closest("input, textarea, select"))) return;
      if (document.querySelector(':modal, [aria-modal="true"]')) return;
      event.preventDefault();
      navigate(returnPath);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [returnPath]);

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader
        crumbs={<span>Workspace</span>}
        title="Settings"
        right={<Segmented options={TABS} value={tab} onChange={(t) => navigate(SETTINGS_PATH[t])} label={(t) => TAB_LABEL[t]} />}
      />
      <PageScroll key={tab}>
        {tab === "general" && <General alerts={alerts} resourcesOn={resourcesOn} setResourcesOn={setResourcesOn} />}
        {(tab === "projects" || tab === "seats" || tab === "system") && <ConfigTabs tab={tab} screen={screen} live={live} />}
        {tab === "usage" && <Usage screen={screen} burnError={burnError} burn={burn} nowMs={nowMs} live={live} />}
        {tab === "quality" && <Quality live={live} />}
        {tab === "updates" && <Updates reload={reload} screen={screen} build={build} view={mainView} live={live} />}
      </PageScroll>
    </main>
  );
}
