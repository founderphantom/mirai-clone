import { Heart, RotateCcw, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { track } from "../lib/analytics";

export type SwipeCard = {
  id: string;
  title: string;
  subtitle?: string;
  imageUrl?: string | null;
};

export function swipeDeckKeyForCards(cards: Array<Pick<SwipeCard, "id">>) {
  return JSON.stringify(cards.map((card) => card.id));
}

export function canAdvanceSwipeDeckAfterAwait(capturedDeckKey: string, currentDeckKey: string) {
  return capturedDeckKey === currentDeckKey;
}

export function SwipeDeck({
  cards,
  emptyLabel = "No cards ready",
  onSwipe
}: {
  cards: SwipeCard[];
  emptyLabel?: string;
  onSwipe?: (card: SwipeCard, verdict: "like" | "dislike") => void | Promise<void>;
}) {
  const [index, setIndex] = useState(0);
  const [pending, setPending] = useState(false);
  const deckKey = useMemo(() => swipeDeckKeyForCards(cards), [cards]);
  const deckKeyRef = useRef(deckKey);
  deckKeyRef.current = deckKey;
  const current = cards[index];
  const remaining = useMemo(() => Math.max(0, cards.length - index), [cards.length, index]);

  useEffect(() => {
    setIndex(0);
  }, [deckKey]);

  async function swipe(verdict: "like" | "dislike") {
    if (!current || pending) return;
    const swipeDeckKey = deckKey;
    setPending(true);
    track("blitz_swipe_preview", { cardId: current.id, verdict });
    try {
      await onSwipe?.(current, verdict);
      if (canAdvanceSwipeDeckAfterAwait(swipeDeckKey, deckKeyRef.current)) {
        setIndex((value) => value + 1);
      }
    } catch {
      return;
    } finally {
      setPending(false);
    }
  }

  if (!current) {
    return (
      <div className="deck-empty">
        <RotateCcw size={24} />
        <p>{emptyLabel}</p>
      </div>
    );
  }

  return (
    <div className="swipe-deck">
      <article className="swipe-card">
        {current.imageUrl ? <img src={current.imageUrl} alt={current.title} /> : <div className="image-placeholder" />}
        <footer>
          <div>
            <h2>{current.title}</h2>
            {current.subtitle && <p>{current.subtitle}</p>}
          </div>
          <span>{remaining}</span>
        </footer>
      </article>
      <div className="swipe-actions">
        <button className="pass" title="Dislike" aria-label="Dislike" disabled={pending} onClick={() => void swipe("dislike")}>
          <X size={24} />
        </button>
        <button className="like" title="Save" aria-label="Save" disabled={pending} onClick={() => void swipe("like")}>
          <Heart size={24} />
        </button>
      </div>
    </div>
  );
}
