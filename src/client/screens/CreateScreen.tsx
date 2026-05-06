import { ImagePlus, Loader2, RefreshCcw, Sparkles } from "lucide-react";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { track } from "../lib/analytics";
import type { Clone, DiscoveryItem, Inspiration } from "../types";

export function CreateScreen({
  clones,
  selectedCloneId,
  onSubmitted
}: {
  clones: Clone[];
  selectedCloneId: string;
  onSubmitted: () => Promise<void>;
}) {
  const [source, setSource] = useState("instagram-reels");
  const [items, setItems] = useState<DiscoveryItem[]>([]);
  const [inspiration, setInspiration] = useState<Inspiration | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const selectedClone = useMemo(
    () => clones.find((clone) => clone.id === selectedCloneId) || clones[0],
    [clones, selectedCloneId]
  );

  async function load(force = false) {
    setBusy(true);
    try {
      const data = force
        ? await api<{ items: DiscoveryItem[] }>("/api/discovery/refresh", {
            method: "POST",
            body: JSON.stringify({ source })
          })
        : await api<{ items: DiscoveryItem[] }>(`/api/discovery/feed?source=${source}`);
      setItems(data.items);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    load().catch(() => setItems([]));
  }, [source]);

  async function upload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    if (selectedClone?.id) form.set("cloneId", selectedClone.id);
    const result = await api<{ media: { id: string } }>("/api/media/upload", { method: "POST", body: form });
    setInspiration({ type: "asset", id: result.media.id, imageUrl: `/api/media/${result.media.id}` });
    track("create_inspiration_uploaded");
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!selectedClone || !inspiration) return;
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setError("");
    try {
      await api("/api/generations", {
        method: "POST",
        body: JSON.stringify({
          cloneId: selectedClone.id,
          prompt: String(form.get("prompt") || "").trim() || undefined,
          inspirationAssetId: inspiration.type === "asset" ? inspiration.id : undefined,
          discoveryItemId: inspiration.type === "discovery" ? inspiration.id : undefined,
          quality: form.get("quality"),
          batchSize: Number(form.get("batchSize") || 4)
        })
      });
      track("generation_queued", { cloneId: selectedClone.id });
      await onSubmitted();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not queue generation.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="create-layout">
      <section className="composer">
        <div className="source-toolbar">
          <select value={source} onChange={(event) => setSource(event.target.value)}>
            <option value="instagram-reels">Instagram</option>
            <option value="tiktok-trending">TikTok</option>
            <option value="youtube-shorts">Shorts</option>
          </select>
          <button className="icon-button" title="Refresh" onClick={() => load(true)} disabled={busy}>
            <RefreshCcw size={18} />
          </button>
        </div>

        <form className="upload-card" onSubmit={upload}>
          <input name="file" type="file" accept="image/*" required />
          <button>
            <ImagePlus size={16} />
            Upload
          </button>
        </form>

        <form onSubmit={submit}>
          {inspiration && <img className="selected-inspiration" src={inspiration.imageUrl} alt="Selected inspiration" />}
          <textarea name="prompt" placeholder="Optional direction" rows={3} />
          <div className="inline-fields">
            <select name="quality" defaultValue="1080p">
              <option value="1080p">1080p</option>
              <option value="2K">2K</option>
            </select>
            <input name="batchSize" type="number" min={1} max={4} defaultValue={4} />
          </div>
          {error && <p className="error">{error}</p>}
          <button className="primary" disabled={!selectedClone || !inspiration || busy}>
            {busy ? <Loader2 className="spin" size={16} /> : <Sparkles size={16} />}
            Generate
          </button>
        </form>
      </section>

      <section className="discovery-grid">
        {items.map((item) => {
          const imageUrl = item.image_url || item.thumbnail_url || "";
          return (
            <button
              className="discovery-card"
              key={item.id}
              onClick={() => {
                setInspiration({ type: "discovery", id: item.id, imageUrl });
                track("create_inspiration_selected", { platform: item.platform });
              }}
            >
              {imageUrl ? <img src={imageUrl} alt={item.title || item.platform} /> : <div className="image-placeholder" />}
              <span>{item.author_handle || item.platform}</span>
            </button>
          );
        })}
      </section>
    </div>
  );
}
