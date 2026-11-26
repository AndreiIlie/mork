import { ago, coldMinutes, wherePlace } from "./format";
import { Cold } from "./icons";
import { OfferButtons, RefusalLine, useHandOffers } from "./offers";
import { cardPath } from "./route";
import type { HandView } from "./types";
import { Dot } from "./ui/Dot";
import { List, Row } from "./ui/List";
import { SectionHead } from "./ui/Section";

const ID = "min-w-[76px] max-w-[220px] shrink-0 truncate font-mono text-label-13 font-medium text-gray-1000";

/** The Board's attention list: the open hands waiting on the operator, in the engine's order, with their offers. */
export function RaisedHands({ hands, nowMs, live }: { hands: HandView[]; nowMs: number; live: boolean }) {
  const { pending, refusals, press } = useHandOffers();
  const expiring = hands.filter((h) => coldMinutes(h.cold_at_ms, nowMs) !== null).length;

  return (
    <section className="mt-gutter">
      <SectionHead
        title="Raised Hands"
        note={`${hands.length} waiting${expiring > 0 ? ` · ${expiring} expiring soon` : ""}`}
        right={<span className="text-label-12 text-gray-700">Red first, then oldest</span>}
      />
      <List>
        {hands.map((h) => {
          const cold = coldMinutes(h.cold_at_ms, nowMs);
          const meta = [wherePlace(h), h.role ?? (h.card === null ? null : "engine"), h.design ? "design" : null].filter(Boolean).join(" · ");
          const card = h.card;
          const actions = card === null ? (
            <OfferButtons
              offers={h.offers}
              live={live}
              pending={pending[h.ask] ?? null}
              onPress={(o) => press(h.ask, o)}
              className="flex items-center gap-2"
            />
          ) : undefined;
          return (
            <Row key={h.ask} to={card !== null ? cardPath(card, h.ask) : undefined} actions={actions}>
              <Dot color={h.color} />
              <span title={card ?? h.place} className={ID}>
                {card ?? h.place}
              </span>
              <div className="min-w-0 flex-1">
                <div className="line-clamp-1 text-label-14 text-gray-1000">{card === null ? h.words : h.headline}</div>
                <div className="mt-0.5 flex flex-wrap items-center gap-x-2 text-label-12 text-gray-700">
                  {meta !== "" && <span>{meta}</span>}
                  <span className="font-mono tabular-nums">{ago(nowMs - h.raised_ms)}</span>
                  {cold !== null && <Cold minutes={cold} />}
                </div>
                <RefusalLine refusal={refusals[h.ask] ?? null} className="mt-1" />
              </div>
            </Row>
          );
        })}
      </List>
    </section>
  );
}
