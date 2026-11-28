import { useEffect } from "react";
import type { AlertPermission } from "./alerts";
import { RefusalLine } from "./offers";
import type { Reload } from "./reload";
import { type SettingsTab, navigate } from "./route";
import type { PlaceView, RecentBurn, Screen } from "./types";
import { Button } from "./ui/Button";
import { Row } from "./ui/List";
import { Segmented } from "./ui/Segmented";
import { ReloadChangelog } from "./ReloadChangelog";
import { Group, Setting } from "./settings/Fields";
import { Projects } from "./settings/Projects";
import { ConfigTabs } from "./settings/ConfigTabs";
import { SettingsPage } from "./settings/Page";
import { Usage } from "./Usage";
import { Quality } from "./Quality";

const ALERT_WORDS = { granted: "Enabled", denied: "Blocked in this browser's site settings", unsupported: "Unavailable in this browser" } as const;

function General({ alerts, resourcesOn, setResourcesOn }: { alerts: AlertPermission; resourcesOn: boolean; setResourcesOn: (on: boolean) => void }) {
  const { permission, request } = alerts;
  return (
    <SettingsPage title="General" words="Choices kept in this browser.">
      <Group title="Notifications">
        <Setting title="Alerts" words="A browser notification when a hand is raised while this tab is in the background.">
          {permission === "default" ? (
            <Button tier="secondary" size="sm" onClick={request}>Enable alerts</Button>
          ) : (
            <span className="text-label-12 text-gray-900">{ALERT_WORDS[permission]}</span>
          )}
        </Setting>
      </Group>
      <Group title="Sidebar">
        <Setting title="Resource tracker" words="CPU and RAM in the sidebar, above the quotas.">
          <Segmented compact options={["Off", "On"] as const} value={resourcesOn ? "On" : "Off"} onChange={(v) => setResourcesOn(v === "On")} />
        </Setting>
      </Group>
    </SettingsPage>
  );
}

function Updates({ screen, build, view, live, reload }: { screen: Screen; build: string | null; view?: PlaceView; live: boolean; reload: Reload }) {
  const pin = screen.top.pin;
  return (
    <SettingsPage title="Updates" words="The build Mork runs, and its reloads.">
      <Group title="Build">
        <Row actions={
          <Button tier={pin.ready ? "primary" : "secondary"} size="sm" title="Reload Mork to its pinned build"
            disabled={!live || reload.busy} onClick={() => reload.press("updates")}>
            Reload
          </Button>
        }>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-label-14 text-gray-1000">Running</span>
              <span className="font-mono text-label-13 text-gray-1000">{build?.slice(0, 8) ?? "offline"}</span>
            </div>
            {pin.ready && (
              <div className="mt-0.5 text-label-12 text-gray-700">Update ready <span className="font-mono text-gray-1000">{pin.pin}</span></div>
            )}
          </div>
        </Row>
      </Group>
      <RefusalLine refusal={reload.refusal("updates")} className="mb-4" />
      <ReloadChangelog view={view} live={live} tzOffsetMin={screen.tz_offset_min} />
    </SettingsPage>
  );
}

export function Settings({ tab, project, returnPath, screen, burn, burnError, build, mainView, nowMs, live, alerts, resourcesOn, setResourcesOn, reload }: {
  reload: Reload;
  tab: SettingsTab;
  project?: string;
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

  if (tab === "general") return <General alerts={alerts} resourcesOn={resourcesOn} setResourcesOn={setResourcesOn} />;
  if (tab === "projects") return <Projects project={project} screen={screen} live={live} />;
  if (tab === "seats") {
    return (
      <SettingsPage title="Seats" words="The model and effort each seat starts on, and what a foreman may give its children.">
        <ConfigTabs live={live} tab={tab} />
      </SettingsPage>
    );
  }
  if (tab === "system") {
    return (
      <SettingsPage title="System" words="Limits, job resources and wall reach shared by every project.">
        <ConfigTabs live={live} tab={tab} />
      </SettingsPage>
    );
  }
  if (tab === "usage") {
    return (
      <SettingsPage title="Usage" words="Spend, account quotas and burn across every project.">
        <Usage screen={screen} burnError={burnError} burn={burn} nowMs={nowMs} live={live} />
      </SettingsPage>
    );
  }
  if (tab === "quality") return <Quality live={live} />;
  return <Updates screen={screen} build={build} view={mainView} live={live} reload={reload} />;
}
