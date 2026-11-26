import { useEffect, useState } from "react";
import { ownHands } from "./alerts";
import { CardControls } from "./CardControls";
import { CardTree, lastKey, stageNodes } from "./CardTree";
import { StageDetail } from "./StageDetail";
import { SeatChat } from "./SeatChat";
import { watchPlace } from "./bridge";
import { agentSeatName, seatPlace } from "./format";
import { cardPath, navigate, replaceRoute } from "./route";
import type { AgentRow, CardHeader, Color, HandView, PlaceView, Screen } from "./types";
import { Button } from "./ui/Button";

// A hand sent to an orchestrator opens whole only when picked; by default the oldest hand waiting on the operator does.
function pickHand(hands: HandView[], own: HandView[], picked: string | null): HandView | undefined {
  return hands.find((h) => h.ask === picked) ?? [...own].sort((a, b) => a.raised_ms - b.raised_ms)[0];
}

function overviewStatus(hands: HandView[], seats: AgentRow[]): Color {
  if (hands.some((h) => h.color === "red") || seats.some((s) => s.light === "error")) return "red";
  if (hands.length > 0) return "yellow";
  if (seats.some((s) => s.light === "working")) return "green";
  return "grey";
}

// A one-room full card's own room is its spec room until its build starts.
function ownRoomLabel(card: CardHeader): string {
  if (card.mode === "review") return "Review";
  if (card.mode === "report") return "Report";
  const atBrief = card.stage === "starting" || card.stage === "brief";
  return card.mode === "full" && card.spec === undefined && atBrief ? "Spec" : "Build";
}

function seatRoomLabel(seat: string, place: string, card: CardHeader, reviewCard: string | undefined): string {
  if (place === card.spec) return "Spec";
  if (place === card.id) return ownRoomLabel(card);
  if (reviewCard !== undefined && (place === reviewCard || seat.startsWith(`${reviewCard}/`))) return "Review";
  return "";
}

interface LoadedProps {
  screen: Screen;
  views: Record<string, PlaceView>;
  buildView: PlaceView;
  card: CardHeader;
  handAsk: string | null;
  nowMs: number;
  live: boolean;
}

function CardRoomLoaded({ screen, views, buildView, card, handAsk, nowMs, live }: LoadedProps) {
  const twoRooms = card.spec !== undefined;
  const review = buildView.review;
  const placeOf = (agent: string) =>
    review !== undefined && agent === `${review.card}/foreman` ? review.card : seatPlace(agent, card.id, twoRooms);
  const reviewForemanRow = review?.foreman ?? null;
  const seats = [
    ...(buildView.sessions ?? []),
    ...(reviewForemanRow !== null ? [reviewForemanRow] : []),
    ...(review?.reviewers ?? []),
    ...(review?.others ?? []),
  ];
  const reviewForeman =
    card.mode === "review" ? `${card.id}/foreman` : review !== undefined ? `${review.card}/foreman` : null;
  const seatName = (agent: string) => (agent === reviewForeman ? "review foreman" : agentSeatName(agent));
  const cardHands = screen.hands.filter((h) => h.card === card.id);
  const own = ownHands(cardHands);
  const [pickedSeat, setPickedSeat] = useState<string | null>(null);
  useEffect(() => {
    if (handAsk !== null) { setPickedSeat(null); setPickedStage(null); }
  }, [handAsk]);
  const [pickedStage, setPickedStage] = useState<string | null>(null);
  const nodes = stageNodes(buildView, card);
  const linked = cardHands.find((h) => h.ask === handAsk);
  const waiting = pickHand(cardHands, own, handAsk);
  const seatAt = (place: string) => {
    const recorded = seats.find((s) => placeOf(s.agent) === place);
    if (recorded !== undefined) return recorded.agent;
    if (place.endsWith("/dm")) return place;
    if (place === card.spec) return `${card.id}/planner`;
    return `${place}/foreman`;
  };
  const handSeat = waiting && seatAt(waiting.message_place);
  const markOf = (agent: string) => {
    const here = cardHands.filter((h) => h.message_place === placeOf(agent));
    if (here.length === 0) return undefined;
    return here.every((h) => h.given) ? "text-gray-700" : "text-gray-1000";
  };

  const pick = (selection: string) => {
    const stage = selection.startsWith("stage ");
    setPickedStage(stage ? selection.slice(6) : null);
    if (!stage) setPickedSeat(selection.slice(5));
    if (handAsk !== null) replaceRoute(cardPath(card.id));
  };

  const step = card.pipeline.find((s) => s.at === "current")?.kind;
  const stageSeat =
    card.mode === "review" || card.mode === "report"
      ? `${card.id}/foreman`
      : twoRooms && step === "spec"
        ? `${card.id}/planner`
        : step === "verify" && review !== undefined && reviewForemanRow !== null
          ? `${review.card}/foreman`
          : `${card.id}/foreman`;
  const openSeat = (linked ? handSeat : pickedSeat) ?? handSeat ?? stageSeat;
  const openPlace = linked?.message_place ?? placeOf(openSeat);
  const extraPlace = openPlace !== card.id && openPlace !== card.spec ? openPlace : null;

  useEffect(() => {
    watchPlace(extraPlace);
    return () => watchPlace(null);
  }, [extraPlace]);

  const stage = !linked && nodes.find((n) => n.key === pickedStage);
  const status = overviewStatus(own, seats);
  const steps = (buildView.overview?.live ?? []).filter((r) => !r.seat);
  const hands = cardHands.filter((h) => h.message_place === openPlace);
  const answering = hands.some((h) => h.kind === "answer" || h.kind === "blocked" && h.role !== null);

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <CardControls card={card} status={status} live={live} />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <CardTree card={card} nodes={nodes} seats={seats} review={review?.card} steps={steps} status={status}
          selected={stage ? `stage ${stage.key}` : `seat ${openSeat}`} pick={pick} seatName={seatName} markOf={markOf} />
        <div className={stage ? "hidden" : "flex min-w-0 flex-1 flex-col overflow-hidden"}>
          <SeatChat
            seat={seats.find((s) => s.agent === openSeat)}
            place={openPlace}
            name={seatName(openSeat)}
            room={seatRoomLabel(openSeat, openPlace, card, review?.card)}
            view={views[openPlace]}
            answering={answering}
            asks={{ hands, triage: buildView.triage, jump: linked?.seq }}
            tzOffsetMin={screen.tz_offset_min}
            nowMs={nowMs}
            live={live}
          />
        </div>
        {stage && <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <StageDetail node={stage} status={status} decisions={buildView.decisions ?? []} triageRound={buildView.triage?.round}
            plan={stage.key === lastKey(nodes, "build", true) ? buildView.plan : undefined}
            steps={stage.at === "current" ? steps : []} tzOffsetMin={screen.tz_offset_min} nowMs={nowMs} />
        </div>}
      </div>
    </main>
  );
}

/** `/cards/<id>`: the card’s stage tree and selected conversation or stage detail. */
export function CardRoom({
  screen,
  views,
  cardId,
  handAsk = null,
  nowMs,
  live,
}: {
  screen: Screen;
  views: Record<string, PlaceView>;
  cardId: string;
  handAsk?: string | null;
  nowMs: number;
  live: boolean;
}) {
  const buildView = views[cardId];
  const parent = buildView?.card?.parent;

  useEffect(() => {
    if (parent !== undefined) replaceRoute(cardPath(parent, handAsk));
  }, [parent, handAsk]);

  if (buildView === undefined || parent !== undefined) {
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center p-8">
        <div className="max-w-[420px] text-center">
          <h1 className="text-heading-16 text-gray-1000">Loading {parent ?? cardId}…</h1>
          <p className="mt-2 text-copy-14 text-gray-900">Waiting for Mork to send the card&apos;s room.</p>
        </div>
      </main>
    );
  }
  if (buildView.card === null) {
    return (
      <main className="flex min-w-0 flex-1 items-center justify-center p-8">
        <div className="max-w-[420px] text-center">
          <h1 className="text-heading-16 text-gray-1000">No room for {cardId}</h1>
          <p className="mt-2 text-copy-14 text-gray-900">This card has no room in Mork.</p>
          <div className="mt-6">
            <Button tier="secondary" onClick={() => navigate("/")}>
              Back to Board
            </Button>
          </div>
        </div>
      </main>
    );
  }
  return (
    <CardRoomLoaded
      key={buildView.card.id}
      screen={screen}
      views={views}
      buildView={buildView}
      card={buildView.card}
      handAsk={handAsk}
      nowMs={nowMs}
      live={live}
    />
  );
}
