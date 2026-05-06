import { AtSign, Camera, Images, Loader2, Sparkles } from "lucide-react";
import { FormEvent, useState } from "react";
import { api } from "../lib/api";
import { track } from "../lib/analytics";

export function OnboardingScreen({ onCreated }: { onCreated: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");

  async function submitStarter(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setError("");
    try {
      await api("/api/clones", {
        method: "POST",
        body: JSON.stringify({
          name: form.get("name") || "Starter Soul",
          handle: form.get("handle") || "starter-soul",
          persona: "Starter creator persona selected during onboarding.",
          stylePrompt: "Lifestyle creator visuals with social-first composition."
        })
      });
      track("onboarding_starter_created");
      await onCreated();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not create starter clone.");
    } finally {
      setBusy(false);
    }
  }

  function captureInstagram(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setNotice(`Instagram source saved: ${String(form.get("instagram") || "").trim()}`);
    track("onboarding_instagram_source_added");
  }

  return (
    <div className="onboarding-grid">
      <section className="moment-card primary-moment">
        <AtSign size={24} />
        <h2>Start from Instagram</h2>
        <form onSubmit={captureInstagram}>
          <input name="instagram" placeholder="instagram.com/handle" required />
          <button className="primary" disabled={busy}>Save source</button>
        </form>
        {notice && <p className="notice">{notice}</p>}
      </section>

      <section className="moment-card">
        <Camera size={24} />
        <h2>Upload photos</h2>
        <p>Manual upload becomes the fallback when Instagram cannot provide enough eligible photos.</p>
      </section>

      <section className="moment-card">
        <Images size={24} />
        <h2>Preset Souls</h2>
        <form onSubmit={submitStarter}>
          <input name="name" placeholder="Clone name" defaultValue="Starter Soul" />
          <input name="handle" placeholder="starter-soul" defaultValue="starter-soul" />
          {error && <p className="error">{error}</p>}
          <button className="primary" disabled={busy}>
            {busy && <Loader2 className="spin" size={16} />}
            <Sparkles size={16} />
            Adopt starter
          </button>
        </form>
      </section>
    </div>
  );
}
