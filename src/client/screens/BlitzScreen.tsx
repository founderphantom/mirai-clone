import { Loader2, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { SwipeDeck, type SwipeCard } from "../components/SwipeDeck";
import { track } from "../lib/analytics";
import { api } from "../lib/api";
import type { BlitzCurrent, Clone } from "../types";

export function BlitzScreen({
  clones,
  selectedCloneId
}: {
  clones: Clone[];
  selectedCloneId: string;
}) {
  const selectedClone = useMemo(
    () => clones.find((clone) => clone.id === selectedCloneId) || clones[0],
    [clones, selectedCloneId]
  );
  const [state, setState] = useState<BlitzCurrent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function load() {
    if (!selectedClone?.id) return;
    setBusy(true);
    setError("");
    try {
      const next = await api<BlitzCurrent>(`/api/blitz/current?clone_id=${encodeURIComponent(selectedClone.id)}`);
      setState(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not load Blitz.");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void load();
  }, [selectedClone?.id]);

  const cards: SwipeCard[] = useMemo(
    () =>
      (state?.batch?.images || [])
        .filter((image) => !image.swiped)
        .map((image) => ({
          id: image.outputId,
          title: selectedClone?.display_name || "Mirai Soul",
          subtitle: `Batch ${state?.batch?.batchNumber || 1}`,
          imageUrl: image.mediaUrl
        })),
    [selectedClone?.display_name, state?.batch]
  );

  async function swipe(card: SwipeCard, verdict: "like" | "dislike") {
    if (!state?.batch) return;
    try {
      await api("/api/blitz/swipe", {
        method: "POST",
        body: JSON.stringify({
          batchId: state.batch.id,
          outputId: card.id,
          action: verdict
        })
      });
      track("blitz_swipe", { verdict, cloneId: selectedClone?.id });
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not save swipe.");
    }
  }

  const readyCount = cards.length;
  const usage = state?.usage;
  const emptyLabel = state?.progress?.detail || "Blitz deck warming up";

  return (
    <div className="screen-stack">
      <section className="app-hero compact">
        <div>
          <span className="app-kicker">Daily Blitz</span>
          <h2>{selectedClone ? `${selectedClone.display_name}'s fresh batch` : "Choose a Soul to Blitz."}</h2>
          <p>{usage ? `${usage.remaining} generations left today.` : "Researching your visual references."}</p>
        </div>
      </section>
      <section className="daily-strip">
        {busy ? <Loader2 className="spin" size={18} /> : <Sparkles size={18} />}
        <span>{readyCount} ready</span>
        {state?.nextBatchStatus && <span>{state.nextBatchStatus}</span>}
      </section>
      {state?.progress && (
        <section className="daily-strip">
          <Sparkles size={18} />
          <span>{state.progress.phase}</span>
          <span>{state.progress.detail}</span>
        </section>
      )}
      {error && <p className="error">{error}</p>}
      <SwipeDeck cards={cards} emptyLabel={emptyLabel} onSwipe={swipe} />
    </div>
  );
}
