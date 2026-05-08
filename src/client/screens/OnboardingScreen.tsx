import { AtSign, Camera, Check, Images, Loader2, Sparkles, WandSparkles } from "lucide-react";
import { motion } from "motion/react";
import type { CSSProperties, FormEvent } from "react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { track } from "../lib/analytics";
import { UploadReferencePanel } from "./onboarding/UploadReferencePanel";
import type {
  Clone,
  InspirationBubble,
  InstagramHarvestJob,
  OnboardingState,
  StarterCharacter
} from "../types";
import { bubbleVisualFor } from "./onboarding-visuals";

type HarvestResponse = {
  job: InstagramHarvestJob;
  clone: Clone | null;
  acceptedAssets?: Array<{ id: string }>;
};

type OnboardingMode = "instagram" | "upload" | "starter" | "bubbles";

const starterImages: Record<string, string> = {
  starter_amara_cherry_grwm: "/landing/starters/starter-amara.jpg",
  starter_priya_resort_edit: "/landing/starters/starter-priya.jpg",
  starter_miles_streetwear_lens: "/landing/starters/starter-miles.jpg",
  starter_hana_seoul_skin: "/landing/starters/starter-hana.jpg",
  starter_leila_fragrance_editorial: "/landing/starters/starter-leila.jpg",
  starter_sky_soft_glam: "/landing/starters/starter-sky.jpg",
  starter_marina_coastal: "/landing/starters/starter-marina.jpg",
  starter_aiden_streetwear: "/landing/starters/starter-aiden.jpg",
  starter_noor_editorial: "/landing/starters/starter-noor.jpg",
  starter_juno_fitness: "/landing/starters/starter-juno.jpg",
  starter_valentin_luxury_travel: "/landing/starters/starter-valentin.jpg",
  starter_sienna_cottagecore: "/landing/starters/starter-sienna.jpg",
  starter_kai_cyber_night: "/landing/starters/starter-kai.jpg",
  starter_maya_minimal_clean: "/landing/starters/starter-maya.jpg",
  starter_rio_festival: "/landing/starters/starter-rio.jpg",
  starter_rowan_parsons_crit: "/landing/starters/starter-rowan.jpg",
  starter_liora_downtown_muse: "/landing/starters/starter-liora.jpg",
  starter_dorian_experimental_dresser: "/landing/starters/starter-dorian.jpg",
  starter_sable_form_study: "/landing/starters/starter-sable.jpg",
  starter_vesper_goth_rap_streetwear: "/landing/starters/starter-vesper.jpg",
  starter_niko_underground_set: "/landing/starters/starter-niko.jpg",
  starter_ren_niche_designer: "/landing/starters/starter-ren.jpg",
  starter_mika_archive_layers: "/landing/starters/starter-mika.jpg"
};

export function OnboardingScreen({ onCreated }: { onCreated: () => Promise<void> }) {
  const [state, setState] = useState<OnboardingState | null>(null);
  const [mode, setMode] = useState<OnboardingMode>("instagram");
  const [activeClone, setActiveClone] = useState<Clone | null>(null);
  const [harvest, setHarvest] = useState<InstagramHarvestJob | null>(null);
  const [selectedBubbles, setSelectedBubbles] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");

  const bubbles = state?.bubbles ?? [];
  const starters = state?.starters ?? [];
  const clone = activeClone ?? state?.activeClone ?? null;
  const canPickBubbles = Boolean(clone?.id);

  useEffect(() => {
    let ignore = false;
    loadState()
      .then((next) => {
        if (ignore) return;
        setState(next);
        setActiveClone(next.activeClone);
        setHarvest(next.latestHarvest);
        setSelectedBubbles(next.bubbles.filter((bubble) => bubble.selected).map((bubble) => bubble.id));
        if (next.bubbles.length > 0) setMode("bubbles");
      })
      .catch(() => undefined);
    return () => {
      ignore = true;
    };
  }, []);

  useEffect(() => {
    if (!harvest || terminalHarvestStatus(harvest.status)) return;
    let ignore = false;
    const timer = window.setInterval(async () => {
      try {
        const response = await api<HarvestResponse>(`/api/onboarding/harvest/${harvest.id}`);
        if (ignore) return;
        setHarvest(response.job);
        if (response.clone) {
          setActiveClone(response.clone);
          await ensureBubbles(response.clone.id);
        }
        if (terminalHarvestStatus(response.job.status)) await refreshState();
      } catch {
        if (!ignore) setError("Instagram harvest is taking longer than expected. Manual upload is ready as a fallback.");
      }
    }, 2400);
    return () => {
      ignore = true;
      window.clearInterval(timer);
    };
  }, [harvest?.id, harvest?.status]);

  const selectedStarter = useMemo(
    () => starters.find((starter) => starter.id === clone?.starter_character_id) ?? null,
    [starters, clone?.starter_character_id]
  );

  async function refreshState() {
    const next = await loadState();
    setState(next);
    if (next.activeClone) setActiveClone(next.activeClone);
    if (next.latestHarvest) setHarvest(next.latestHarvest);
    setSelectedBubbles(next.bubbles.filter((bubble) => bubble.selected).map((bubble) => bubble.id));
    return next;
  }

  async function startInstagram(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const instagram = String(form.get("instagram") || "").trim();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const response = await api<{ job: InstagramHarvestJob }>("/api/onboarding/instagram", {
        method: "POST",
        body: JSON.stringify({ instagram })
      });
      setHarvest(response.job);
      setNotice("Harvest started. Mirai is checking public posts for clear reference photos.");
      track("onboarding_instagram_started", { handle: response.job.handle });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not start Instagram harvest.");
    } finally {
      setBusy(false);
    }
  }

  async function uploadPhotos(form: FormData) {
    setBusy(true);
    setError("");
    try {
      const response = await api<{ clone: Clone }>("/api/onboarding/upload", {
        method: "POST",
        body: form
      });
      setActiveClone(response.clone);
      await ensureBubbles(response.clone.id);
      setMode("bubbles");
      setNotice("Reference photos saved. Your Soul is waiting for the creation script.");
      track("onboarding_upload_created", { cloneId: response.clone.id });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not upload reference photos.");
    } finally {
      setBusy(false);
    }
  }

  async function adoptStarter(starter: StarterCharacter) {
    setBusy(true);
    setError("");
    try {
      const response = await api<{ clone: Clone }>("/api/onboarding/starter", {
        method: "POST",
        body: JSON.stringify({ starterId: starter.id })
      });
      setActiveClone(response.clone);
      await ensureBubbles(response.clone.id);
      setMode("bubbles");
      setNotice(`${starter.name} is adopted. You can swap to your own Instagram or uploads later.`);
      track("onboarding_starter_adopted", { starterId: starter.id });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not adopt this Starter Soul.");
    } finally {
      setBusy(false);
    }
  }

  async function ensureBubbles(cloneId: string) {
    const response = await api<{ bubbles: InspirationBubble[] }>("/api/onboarding/bubbles/generate", {
      method: "POST",
      body: JSON.stringify({ cloneId })
    });
    setState((current) => current ? { ...current, bubbles: response.bubbles } : current);
    setSelectedBubbles(response.bubbles.filter((bubble) => bubble.selected).map((bubble) => bubble.id));
    setMode("bubbles");
  }

  async function submitBubbles() {
    if (!clone) return;
    setBusy(true);
    setError("");
    try {
      await api("/api/onboarding/bubbles", {
        method: "POST",
        body: JSON.stringify({ cloneId: clone.id, bubbleIds: selectedBubbles })
      });
      track("onboarding_bubbles_selected", { count: selectedBubbles.length });
      await onCreated();
      setNotice("Inspiration bubbles saved. Your personal pool is seeding in the background.");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not save inspiration bubbles.");
    } finally {
      setBusy(false);
    }
  }

  function toggleBubble(id: string) {
    setSelectedBubbles((current) => {
      if (current.includes(id)) return current.filter((value) => value !== id);
      if (current.length >= 5) return current;
      return [...current, id];
    });
  }

  return (
    <div className="onboarding-flow">
      <section className="app-hero onboarding-hero onboarding-dashboard-hero">
        <div className="onboarding-hero-copy">
          <span className="app-kicker">Creator setup</span>
          <h2>Shape the first version of your creator workspace.</h2>
          <p>Start from the source that best captures your look, then choose the visual lane Mirai should lean into first.</p>
          <div className="onboarding-hero-pills" aria-label="Available setup steps">
            <span>Instagram</span>
            <span>Uploads</span>
            <span>Starters</span>
            <span>Bubbles</span>
          </div>
        </div>
        <div className="onboarding-hero-visual" aria-hidden="true">
          <img className="onboarding-hero-main" src="/landing/clone-y2k-cafe.jpg" alt="" />
          <img className="onboarding-hero-float" src="/landing/clone-tokyo-neon.jpg" alt="" />
          <div className="onboarding-hero-status">
            <span>{selectedBubbles.length}/5</span>
            <small>bubbles selected</small>
          </div>
        </div>
      </section>

      <section className="source-tabs onboarding-source-tabs" aria-label="Soul source">
        <button type="button" className={sourceTabClass(mode, "instagram", "source-tab-instagram")} onClick={() => setMode("instagram")}>
          <AtSign size={18} />
          <strong>Instagram</strong>
          <span>Public profile</span>
        </button>
        <button type="button" className={sourceTabClass(mode, "upload", "source-tab-upload")} onClick={() => setMode("upload")}>
          <Camera size={18} />
          <strong>Upload</strong>
          <span>Reference photos</span>
        </button>
        <button type="button" className={sourceTabClass(mode, "starter", "source-tab-starter")} onClick={() => setMode("starter")}>
          <Images size={18} />
          <strong>Starters</strong>
          <span>Preset creators</span>
        </button>
        <button type="button" className={sourceTabClass(mode, "bubbles", "source-tab-bubbles")} disabled={!canPickBubbles} onClick={() => setMode("bubbles")}>
          <WandSparkles size={18} />
          <strong>Bubbles</strong>
          <span>Visual direction</span>
        </button>
      </section>

      {clone && (
        <section className="pending-script">
          <Sparkles size={16} />
          <div>
            <strong>{clone.name}</strong>
            <span>Soul Character creation pending script</span>
          </div>
        </section>
      )}

      {mode === "instagram" && (
        <motion.section className="moment-card primary-moment" initial={{ y: 12, opacity: 0 }} animate={{ y: 0, opacity: 1 }}>
          <AtSign size={24} />
          <h2>Paste your Instagram</h2>
          <p>Mirai will scan public posts and save up to 12 likely reference photos. If fewer than 5 are usable, upload photos instead.</p>
          <form className="source-form" onSubmit={startInstagram}>
            <input name="instagram" placeholder="instagram.com/handle" required />
            <button className="primary" disabled={busy}>
              {busy ? <Loader2 className="spin" size={16} /> : <Sparkles size={16} />}
              Start harvest
            </button>
          </form>
          {harvest && <HarvestProgress job={harvest} />}
        </motion.section>
      )}

      {mode === "upload" && (
        <motion.section initial={{ y: 12, opacity: 0 }} animate={{ y: 0, opacity: 1 }}>
          <UploadReferencePanel busy={busy} onSubmit={uploadPhotos} />
        </motion.section>
      )}

      {mode === "starter" && (
        <motion.section className="starter-section" initial={{ y: 12, opacity: 0 }} animate={{ y: 0, opacity: 1 }}>
          <div className="app-section-title">
            <div>
              <span className="app-kicker">Preset Souls</span>
              <h2>Pick a Starter while your own Soul setup waits.</h2>
            </div>
          </div>
          <div className="starter-grid">
            {starters.map((starter, index) => (
              <button className="starter-card" key={starter.id} onClick={() => adoptStarter(starter)} disabled={busy}>
                <div className="starter-card-media">
                  <img src={starterImages[starter.id] || starterImages[starter.slug] || fallbackStarterImage(index)} alt={starter.name} />
                  <div className="starter-card-overlay">
                    <span>{starter.status === "setup_pending" ? "Setup pending" : "Ready"}</span>
                    <strong>{starter.name}</strong>
                    <p>{starter.persona}</p>
                  </div>
                </div>
              </button>
            ))}
          </div>
        </motion.section>
      )}

      {mode === "bubbles" && (
        <motion.section className="moment-card onboarding-bubbles-card" initial={{ y: 12, opacity: 0 }} animate={{ y: 0, opacity: 1 }}>
          <div className="onboarding-card-header">
            <div>
              <WandSparkles size={24} />
              <h2>Choose up to 5 inspiration bubbles</h2>
              <p>These seed ScrapeCreators searches for your personal inspiration pool.</p>
            </div>
            <span>{selectedBubbles.length}/5</span>
          </div>
          <div className="bubble-grid onboarding-bubble-grid">
            {bubbles.map((bubble) => {
              const active = selectedBubbles.includes(bubble.id);
              const visual = bubbleVisualFor(bubble);
              const style = { "--bubble-image": `url(${visual.src})` } as CSSProperties;
              return (
                <button
                  aria-label={`${active ? "Remove" : "Select"} ${bubble.title} inspiration bubble`}
                  aria-pressed={active}
                  className={active ? "bubble-chip onboarding-bubble selected" : "bubble-chip onboarding-bubble"}
                  key={bubble.id}
                  onClick={() => toggleBubble(bubble.id)}
                  style={style}
                  type="button"
                >
                  <span className="bubble-checkmark" aria-hidden="true">{active && <Check size={14} />}</span>
                  <span className="bubble-copy">
                    <small>{visual.label}</small>
                    <strong>{bubble.title}</strong>
                    <span className="bubble-summary">{bubble.vibe_summary}</span>
                  </span>
                </button>
              );
            })}
          </div>
          <button className="primary" disabled={busy || selectedBubbles.length === 0} onClick={submitBubbles}>
            {busy ? <Loader2 className="spin" size={16} /> : <Sparkles size={16} />}
            Save {selectedBubbles.length || ""} bubbles
          </button>
        </motion.section>
      )}

      {selectedStarter && <p className="notice">Starter selected: {selectedStarter.name}</p>}
      {notice && <p className="notice">{notice}</p>}
      {error && <p className="error">{error}</p>}
    </div>
  );
}

async function loadState() {
  return await api<OnboardingState>("/api/onboarding/state");
}

function terminalHarvestStatus(status: string): boolean {
  return ["ready_for_soul_script", "failed", "canceled"].includes(status);
}

function fallbackStarterImage(index: number) {
  const images = Object.values(starterImages);
  return images[index % images.length];
}

function sourceTabClass(mode: OnboardingMode, tab: OnboardingMode, imageClass: string) {
  return `source-tab ${imageClass}${mode === tab ? " active" : ""}`;
}

function HarvestProgress({ job }: { job: InstagramHarvestJob }) {
  const percent = job.candidate_count > 0 ? Math.min(100, Math.round((job.accepted_count / 5) * 100)) : 12;
  return (
    <div className="progress-rail">
      <div>
        <strong>@{job.handle}</strong>
        <span>{statusLabel(job)}</span>
      </div>
      <div className="progress-track">
        <span style={{ width: `${percent}%` }} />
      </div>
      {job.fail_reason && <p>{job.fail_reason.replaceAll("_", " ")}</p>}
    </div>
  );
}

function statusLabel(job: InstagramHarvestJob): string {
  if (job.status === "queued") return "Queued for public profile scan";
  if (job.status === "scraping") return "Collecting recent posts";
  if (job.status === "filtering") return `${job.accepted_count}/5 eligible photos saved`;
  if (job.status === "ready_for_soul_script") return "Ready for the Soul creation script";
  if (job.status === "failed") return "Needs manual upload fallback";
  return job.status.replaceAll("_", " ");
}
