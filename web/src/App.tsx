import { useCallback, useEffect, useState } from "react";
import { Board } from "./Board";
import { CardRoom } from "./CardRoom";
import { Sidebar } from "./Sidebar";
import { pruneTriageState } from "./Triage";
import { Usage } from "./Usage";
import { useHandAlerts, useNotificationPermission } from "./alerts";
import { type BridgeState, INITIAL_STATE, connectBridge, watchCard } from "./bridge";
import { useRecentBurn } from "./burn";
import { type Route, cardPath, navigate, parseRoute } from "./route";
import type { HandView } from "./types";
import { Dot } from "./ui/Dot";

const TICK_MS = 5000;

function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(window.location.pathname, window.location.search));
  useEffect(() => {
    const onPop = () => setRoute(parseRoute(window.location.pathname, window.location.search));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  return route;
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
  const route = useRoute();
  const nowMs = useNow(state);
  const phase = phaseWords(state);
  const activeCard = route.page === "card" ? route.id : null;
  const alerts = useNotificationPermission();
  const { burn, error: burnError } = useRecentBurn(state.link === "live");

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
  else if (route.page === "usage") {
    page = <Usage screen={state.screen} burn={burn} burnError={burnError} nowMs={nowMs} live={state.link === "live"} />;
  } else if (route.page === "card") {
    page = (
      <CardRoom
        screen={state.screen}
        views={state.views}
        cardId={route.id}
        handAsk={route.hand}
        nowMs={nowMs}
        live={state.link === "live"}
      />
    );
  } else page = <Board screen={state.screen} burn={burn} nowMs={nowMs} live={state.link === "live"} />;

  return (
    <div className="flex h-screen bg-background-100 text-label-14 text-gray-1000">
      <Sidebar
        screen={state.screen}
        link={state.link}
        build={state.build}
        activeCard={activeCard}
        onUsage={route.page === "usage"}
        nowMs={nowMs}
        notifications={alerts}
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
