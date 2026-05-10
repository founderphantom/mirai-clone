import { Loader2 } from "lucide-react";
import { FormEvent, useState } from "react";
import { api } from "../lib/api";
import type { Clone } from "../types";
import { validateReferenceFiles } from "./onboarding/upload-reference-guidance";

export function ClonesScreen({ clones, onCreated }: { clones: Clone[]; onCreated: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const validation = validateReferenceFiles(form.getAll("photos").filter((file): file is File => file instanceof File));
    if (!validation.valid) {
      setError(validation.message);
      return;
    }
    setBusy(true);
    setError("");
    try {
      await api("/api/clones/manual-upload", {
        method: "POST",
        body: form
      });
      event.currentTarget.reset();
      await onCreated();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not create clone.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="clones-screen">
      <form className="clone-form" onSubmit={submit}>
        <input name="displayName" placeholder="Soul name" required />
        <input name="photos" type="file" accept="image/*" multiple required />
        {error && <p className="error">{error}</p>}
        <button className="primary" disabled={busy}>
          {busy && <Loader2 className="spin" size={16} />}
          Create clone
        </button>
      </form>
      <section className="clone-list">
        {clones.map((clone) => (
          <article key={clone.id}>
            <h2>{cloneDisplayName(clone)}</h2>
            <p>@{clone.handle}</p>
          </article>
        ))}
      </section>
    </div>
  );
}

function cloneDisplayName(clone: Clone) {
  return clone.display_name || clone.handle || "Mirai Soul";
}
