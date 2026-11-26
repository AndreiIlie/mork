import { useEffect, useState } from "react";
import { ownHands } from "./alerts";
import { Overview } from "./Overview";
import { Pipeline } from "./Pipeline";
import { HandIcon } from "./icons";
import { SeatChat, SeatDot } from "./SeatChat";
import { watchPlace } from "./bridge";
import { agentSeatName, seatPlace, thousands } from "./format";
import { cardPath, navigate, replaceRoute } from "./route";
import type { AgentRow, CardHeader, Color, HandView, PlaceView, Screen } from "./types";
import { Button } from "./ui/Button";
import { PageHeader } from "./ui/Page";
import { SplitPane } from "./ui/SplitPane";
import { TextLink } from "./ui/TextLink";

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

function SeatSwitcher({
  seats,
  open,
  nameOf,
  markOf,
  onOpen,
}: {
  seats: AgentRow[];
  open: string;
  nameOf: (agent: string) => string;
  markOf: (agent: string) => string | undefined;
  onOpen: (agent: string) => void;
}) {
  if (seats.length === 0) return null;
  return (
    <div className="mt-3 flex flex-wrap items-center gap-2">
      <span className="text-label-12 text-gray-700">Seats</span>
      {seats.map((s) => (
        <Button key={s.agent} tier="secondary" size="sm" pressed={s.agent === open} onClick={() => onOpen(s.agent)}>
          <SeatDot light={s.light} />
          {markOf(s.agent) && <span aria-label="Raised Hand" className={markOf(s.agent)}><HandIcon /></span>}
          <span className="font-mono font-medium text-gray-1000">{nameOf(s.agent)}</span>
          <span className="font-normal text-gray-700">
            {s.label}
            {s.context !== null ? ` · ${thousands(s.context[0])}` : ""}
          </span>
        </Button>
      ))}
    </div>
  );
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
    if (here.every((h) => h.given)) return "text-gray-700";
    return here.some((h) => !h.given && h.color === "red") ? "text-red-700" : "text-amber-700";
  };

  const onPickSeat = (agent: string) => {
    setPickedSeat(agent);
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

  const ownRoom = ownRoomLabel(card);
  const hands = cardHands.filter((h) => h.message_place === openPlace);
  const answering = hands.some((h) => h.kind === "answer" || h.kind === "blocked" && h.role !== null);

  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <PageHeader
        crumbs={
          <>
            <TextLink to="/">Board</TextLink>
            {card.about.project !== "" && (
              <>
                <span>/</span>
                <span>{card.about.project}</span>
              </>
            )}
            <span>/</span>
            <span className="font-mono text-gray-1000">{card.id}</span>
            {card.paused && <span className="font-mono text-label-12 text-gray-700">paused</span>}
          </>
        }
        title={card.title}
      >
        {card.pipeline.length > 0 && <Pipeline steps={card.pipeline} />}
        <SeatSwitcher seats={seats} open={openSeat} nameOf={seatName} markOf={markOf} onOpen={onPickSeat} />
      </PageHeader>
      <SplitPane
        left={
          <Overview
            overview={buildView.overview}
            plan={buildView.plan}
            triageRound={buildView.triage?.round}
            tzOffsetMin={screen.tz_offset_min}
            status={overviewStatus(own, seats)}
            nowMs={nowMs}
          />
        }
        right={
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
        }
      />
    </main>
  );
}

/** `/cards/<id>`: 50/50 split Card Room with Overview on the left and active Seat Chat on the right. */
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
