import { Heart, RotateCcw, X } from "lucide-react";
import { useMemo, useState } from "react";
import { track } from "../lib/analytics";

export type SwipeCard = {
  id: string;
  title: string;
  subtitle?: string;
  imageUrl?: string | null;
};

export function SwipeDeck({
  cards,
  emptyLabel = "No cards ready",
  onSwipe
}: {
  cards: SwipeCard[];
  emptyLabel?: string;
  onSwipe?: (card: SwipeCard, verdict: "like" | "pass") => void;
}) {
  const [index, setIndex] = useState(0);
  const current = cards[index];
  const remaining = useMemo(() => Math.max(0, cards.length - index), [cards.length, index]);

  function swipe(verdict: "like" | "pass") {
    if (!current) return;
    track("blitz_swipe_preview", { cardId: current.id, verdict });
    onSwipe?.(current, verdict);
    setIndex((value) => value + 1);
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
        <button className="pass" title="Pass" onClick={() => swipe("pass")}>
          <X size={24} />
        </button>
        <button className="like" title="Save" onClick={() => swipe("like")}>
          <Heart size={24} />
        </button>
      </div>
    </div>
  );
}
