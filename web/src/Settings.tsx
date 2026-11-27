import { type ReactNode, useEffect } from "react";
import { ReloadChangelog } from "./ReloadChangelog";
import { Usage } from "./Usage";
import { Quality } from "./Quality";
import type { AlertPermission } from "./alerts";
import { RefusalLine, useHandOffers } from "./offers";
import { SETTINGS_PATH, type SettingsTab, navigate } from "./route";
import type { PlaceView, RecentBurn, Screen } from "./types";
import { Button } from "./ui/Button";
import { List, Row } from "./ui/List";
import { PageHeader, PageScroll } from "./ui/Page";
import { Segmented } from "./ui/Segmented";

const TABS = ["general", "usage", "quality", "updates"] as const;
const TAB_LABEL: Record<SettingsTab, string> = { general: "General", usage: "Usage", quality: "Quality", updates: "Updates" };
const ALERT_WORDS = { granted: "Enabled", denied: "Blocked in this browser's site settings", unsupported: "Unavailable in this browser" } as const;

function Setting({ title, words, children }: { title: string; words: string; children: ReactNode }) {
  return (
    <Row actions={children}>
      <div className="min-w-0 flex-1">
        <div className="text-label-14 text-gray-1000">{title}</div>
        <div className="mt-0.5 text-label-12 text-gray-700">{words}</div>
      </div>
    </Row>
  );
}

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

function Updates({ screen, build, view, live }: { screen: Screen; build: string | null; view?: PlaceView; live: boolean }) {
  const pin = screen.top.pin;
  const { pending, refusals, press } = useHandOffers();
  const busy = pending.reload !== undefined || screen.top.phase.is === "reloading";
  return (
    <>
      <List>
        <Row actions={
          <Button tier={pin.ready ? "primary" : "secondary"} size="sm" title="Reload Mork to its pinned build"
            disabled={!live || busy} onClick={() => press("reload", { label: "Reload", command: { reload: {} } })}>
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
      <RefusalLine refusal={refusals.reload ?? null} className="mt-2" />
      <ReloadChangelog view={view} live={live} tzOffsetMin={screen.tz_offset_min} />
    </>
  );
}

export function Settings({ tab, returnPath, screen, burn, burnError, build, mainView, nowMs, live, alerts, resourcesOn, setResourcesOn }: {
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
        {tab === "usage" && <Usage screen={screen} burnError={burnError} burn={burn} nowMs={nowMs} live={live} />}
        {tab === "quality" && <Quality live={live} />}
        {tab === "updates" && <Updates screen={screen} build={build} view={mainView} live={live} />}
      </PageScroll>
    </main>
  );
}
