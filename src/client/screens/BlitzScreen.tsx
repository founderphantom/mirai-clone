import { Loader2, Sparkles } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { SwipeDeck, type SwipeCard } from "../components/SwipeDeck";
import { track } from "../lib/analytics";
import { api } from "../lib/api";
import type { BlitzCurrent, Clone } from "../types";

export type LoadedBlitzState = {
  cloneId: string;
  data: BlitzCurrent;
};

export function isLoadedBlitzStateForClone(
  loaded: LoadedBlitzState | null,
  cloneId: string | undefined
): loaded is LoadedBlitzState {
  return Boolean(loaded && cloneId && loaded.cloneId === cloneId);
}

function loadBlitzCurrent(cloneId: string) {
  return api<BlitzCurrent>(`/api/blitz/current?clone_id=${encodeURIComponent(cloneId)}`);
}

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
  const selectedCloneIdForRender = selectedClone?.id || "";
  const currentCloneIdRef = useRef(selectedCloneIdForRender);
  const [loaded, setLoaded] = useState<LoadedBlitzState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    currentCloneIdRef.current = selectedCloneIdForRender;
    setLoaded(null);
    setError("");
    if (!selectedCloneIdForRender) {
      setBusy(false);
      return;
    }

    let ignore = false;
    setBusy(true);
    loadBlitzCurrent(selectedCloneIdForRender)
      .then((data) => {
        if (!ignore && currentCloneIdRef.current === selectedCloneIdForRender) {
          setLoaded({ cloneId: selectedCloneIdForRender, data });
        }
      })
      .catch((err) => {
        if (!ignore && currentCloneIdRef.current === selectedCloneIdForRender) {
          setError(err instanceof Error ? err.message : "Could not load Blitz.");
        }
      })
      .finally(() => {
        if (!ignore && currentCloneIdRef.current === selectedCloneIdForRender) {
          setBusy(false);
        }
      });

    return () => {
      ignore = true;
    };
  }, [selectedCloneIdForRender]);

  const activeState = isLoadedBlitzStateForClone(loaded, selectedCloneIdForRender) ? loaded.data : null;

  const cards: SwipeCard[] = useMemo(
    () =>
      (activeState?.batch?.images || [])
        .filter((image) => !image.swiped)
        .map((image) => ({
          id: image.outputId,
          title: selectedClone?.display_name || "Mirai Soul",
          subtitle: `Batch ${activeState?.batch?.batchNumber || 1}`,
          imageUrl: image.mediaUrl
        })),
    [activeState?.batch, selectedClone?.display_name]
  );

  async function swipe(card: SwipeCard, verdict: "like" | "dislike") {
    const cloneId = selectedClone?.id;
    if (!cloneId || !loaded?.data.batch || !isLoadedBlitzStateForClone(loaded, cloneId)) return;
    try {
      await api("/api/blitz/swipe", {
        method: "POST",
        body: JSON.stringify({
          batchId: loaded.data.batch.id,
          outputId: card.id,
          action: verdict
        })
      });
      track("blitz_swipe", { verdict, cloneId });
      const data = await loadBlitzCurrent(cloneId);
      if (currentCloneIdRef.current === cloneId) {
        setLoaded({ cloneId, data });
      }
    } catch (err) {
      if (currentCloneIdRef.current === cloneId) {
        setError(err instanceof Error ? err.message : "Could not save swipe.");
      }
      throw err;
    }
  }

  const readyCount = cards.length;
  const usage = activeState?.usage;
  const emptyLabel = activeState?.progress?.detail || "Blitz deck warming up";

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
        {activeState?.nextBatchStatus && <span>{activeState.nextBatchStatus}</span>}
      </section>
      {activeState?.progress && (
        <section className="daily-strip">
          <Sparkles size={18} />
          <span>{activeState.progress.phase}</span>
          <span>{activeState.progress.detail}</span>
        </section>
      )}
      {error && <p className="error">{error}</p>}
      <SwipeDeck cards={cards} emptyLabel={emptyLabel} onSwipe={swipe} />
    </div>
  );
}
