import { useState } from "react";
import { sendCommand } from "./bridge";
import type { Offer } from "./types";
import { Button, type ButtonTier } from "./ui/Button";

function offerTier(label: string): ButtonTier {
  if (label === "merge" || label === "approve") return "primary";
  if (label === "resume" || label === "retry") return "secondary";
  return "ghost";
}

function without(record: Record<string, string>, key: string): Record<string, string> {
  const next = { ...record };
  delete next[key];
  return next;
}

/** Shared pending and refusal tracking for pressing an open hand's offer buttons. */
export function useHandOffers() {
  const [pending, setPending] = useState<Record<string, string>>({});
  const [refusals, setRefusals] = useState<Record<string, string>>({});

  const press = (ask: string, offer: Offer, onSuccess?: () => void) => {
    setPending((p) => ({ ...p, [ask]: offer.label }));
    setRefusals((r) => without(r, ask));
    sendCommand(offer.command)
      .then(onSuccess)
      .catch((e: unknown) => {
        const why = e instanceof Error ? e.message : String(e);
        setRefusals((r) => ({ ...r, [ask]: `${offer.label}: ${why}` }));
      })
      .finally(() => setPending((p) => without(p, ask)));
  };

  return { pending, refusals, press };
}

/** Shared offer button strip with disabled-while-pending and `label…` feedback. */
export function OfferButtons({
  offers,
  live,
  pending,
  onPress,
  className,
}: {
  offers: Offer[];
  live: boolean;
  pending: string | null;
  onPress: (offer: Offer) => void;
  className: string;
}) {
  if (offers.length === 0) return null;
  return (
    <div className={className}>
      {offers.map((o) => (
        <Button
          key={o.label}
          tier={offerTier(o.label)}
          size="sm"
          disabled={!live || pending !== null}
          onClick={() => onPress(o)}
        >
          {pending === o.label ? `${o.label}…` : o.label}
        </Button>
      ))}
    </div>
  );
}

/** Shared inline refusal line rendered in red under a hand whose offer was refused. */
export function RefusalLine({ refusal, className = "mt-2 pl-5" }: { refusal: string | null; className?: string }) {
  if (refusal === null) return null;
  return (
    <p role="alert" className={`${className} text-copy-13 text-red-900`}>
      {refusal}
    </p>
  );
}
