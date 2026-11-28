import { useCallback, useEffect, useState } from "react";
import { CardRoom } from "./CardRoom";
import { Console } from "./Console";
import { Project } from "./Project";
import { Settings } from "./Settings";
import { Sidebar } from "./Sidebar";
import { pruneTriageState } from "./Triage";
import { useHandAlerts, useNotificationPermission } from "./alerts";
import { type BridgeState, INITIAL_STATE, connectBridge, watchCard } from "./bridge";
import { useRecentBurn } from "./burn";
import { useResourceTracker } from "./prefs";
import { TextLink } from "./ui/TextLink";
import { cardPath, navigate, parseRoute } from "./route";
import { useReload } from "./reload";
import type { HandView } from "./types";
import { Dot } from "./ui/Dot";

const TICK_MS = 5000;

function readRoute() {
  const { pathname, search } = window.location;
  return { route: parseRoute(pathname, search), address: pathname + search };
}

function useRoute() {
  const [location, setLocation] = useState(() => {
    const { route, address } = readRoute();
    return { route, returnPath: route.page === "settings" ? "/" : address };
  });
  useEffect(() => {
    const onPop = () => {
      const { route, address } = readRoute();
      setLocation((previous) => ({ route, returnPath: route.page === "settings" ? previous.returnPath : address }));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  return location;
}

// The engine's clock at its last screen, run on by the browser's, so countdowns move between screens.
function useNow(state: BridgeState): number {
  const [tick, setTick] = useState(() => Date.now());
  const ticking = state.screen !== null;
  useEffect(() => {
    if (!ticking) return;
    setTick(Date.now());
    const timer = window.setInterval(() => setTick(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [ticking, state.screenAt]);
  if (state.screen === null) return tick;
  return state.screen.now_ms + Math.max(0, tick - state.screenAt);
}

// A dial's own `recovering` answer is fresher than the phase of a screen kept from before a drop.
function phaseWords(state: BridgeState): string | null {
  if (state.recovering !== null) return state.recovering.charAt(0).toUpperCase() + state.recovering.slice(1);
  const phase = state.screen?.top.phase;
  if (phase?.is === "recovering") return phase.doing === "" ? "Mork is recovering" : `Mork is recovering: ${phase.doing}`;
  if (phase?.is === "reloading") return `Mork is reloading to ${phase.short}`;
  return null;
}

function Banner({ children }: { children: string }) {
  return (
    <div role="status" className="flex min-h-9 shrink-0 items-center gap-3 border-b border-gray-400 px-8 py-2 text-label-13 text-gray-1000">
      <Dot color="yellow" pulse />
      {children}
    </div>
  );
}

function Waiting({ state }: { state: BridgeState }) {
  const disconnected = state.link === "disconnected";
  return (
    <main className="flex min-w-0 flex-1 items-center justify-center p-8">
      <div className="max-w-[420px] text-center">
        <h1 className="text-heading-16 text-gray-1000">{disconnected ? "Not connected" : "Connecting…"}</h1>
        <p className="mt-2 text-copy-14 text-gray-900">
          {disconnected ? (
            <>
              Open the link <span className="font-mono text-gray-1000">mork web</span> prints.
            </>
          ) : (
            "Waiting for Mork to send its board."
          )}
        </p>
      </div>
    </main>
  );
}

export function App() {
  const [state, setState] = useState<BridgeState>(INITIAL_STATE);
  const { route, returnPath } = useRoute();
  const nowMs = useNow(state);
  const phase = phaseWords(state);
  const activeCard = route.page === "card" ? route.id : null;
  const [resourcesOn, setResourcesOn] = useResourceTracker();
  const live = state.link === "live";
  const alerts = useNotificationPermission();
  const { burn, error: burnError } = useRecentBurn(live);
  const reload = useReload(state.screen?.top.phase.is === "reloading");

  const onOpenHand = useCallback((hand: HandView) => {
    if (hand.card === null) {
      navigate("/");
      return;
    }
    navigate(cardPath(hand.card, hand.ask));
  }, []);

  useHandAlerts(state.screen, state.link, onOpenHand);
  useEffect(() => {
    if (state.screen === null) return;
    pruneTriageState(new Set(state.screen.hands.filter((h) => h.kind === "triage").map((h) => h.ask)));
  }, [state.screen]);
  useEffect(() => watchCard(activeCard), [activeCard]);
  useEffect(() => connectBridge(setState), []);

  let page;
  if (state.screen === null) page = <Waiting state={state} />;
  else if (route.page === "settings") {
    page = <Settings
      tab={route.tab} returnPath={returnPath} screen={state.screen} burn={burn} burnError={burnError}
      build={state.build} mainView={state.views.main} nowMs={nowMs} live={live}
      alerts={alerts} resourcesOn={resourcesOn} setResourcesOn={setResourcesOn} reload={reload}
    />;
  } else if (route.page === "project") {
    const board = state.screen.boards.find((b) => b.project === route.project);
    page = board ? (
      <Project key={board.project} board={board} lookback={state.screen.projects?.find((p) => p.project === board.project)}
        views={state.views} nowMs={nowMs} live={live} />
    ) : (
      <main className="flex min-w-0 flex-1 flex-col items-center justify-center gap-3">
        <h1 className="text-heading-16">No project {route.project}</h1>
        <TextLink to="/">Back to Mork Console</TextLink>
      </main>
    );
  } else if (route.page === "card") {
    page = (
      <CardRoom
        screen={state.screen}
        views={state.views}
        cardId={route.id}
        handAsk={route.hand}
        nowMs={nowMs}
        live={live}
      />
    );
  } else page = <Console screen={state.screen} burnError={burnError} burn={burn} nowMs={nowMs} live={live} />;

  return (
    <div className="flex h-screen bg-background-100 text-label-14 text-gray-1000">
      <Sidebar
        screen={state.screen}
        link={state.link}
        build={state.build}
        route={route}
        returnPath={returnPath}
        reload={reload}
        nowMs={nowMs}
        resourcesOn={resourcesOn}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        {state.link === "reconnecting" && (
          <Banner>Reconnecting… showing the last board Mork sent; its buttons wait.</Banner>
        )}
        {phase !== null && <Banner>{phase}</Banner>}
        <div className="flex min-h-0 flex-1">{page}</div>
      </div>
    </div>
  );
}
