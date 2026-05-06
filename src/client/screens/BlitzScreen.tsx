import { Sparkles } from "lucide-react";
import { SwipeDeck } from "../components/SwipeDeck";
import type { Job } from "../types";

export function BlitzScreen({ jobs }: { jobs: Job[] }) {
  const cards = jobs
    .filter((job) => job.status === "completed" && job.preview_media_id)
    .slice(0, 12)
    .map((job) => ({
      id: job.id,
      title: job.clone_name || "Mirai clone",
      subtitle: job.prompt || "Fresh clone",
      imageUrl: `/api/media/${job.preview_media_id}`
    }));

  return (
    <div className="screen-stack">
      <section className="daily-strip">
        <Sparkles size={18} />
        <span>{cards.length || 0} ready</span>
      </section>
      <SwipeDeck cards={cards} emptyLabel="Blitz deck warming up" />
    </div>
  );
}
