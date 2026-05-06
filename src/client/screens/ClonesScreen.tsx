import { Loader2 } from "lucide-react";
import { FormEvent, useState } from "react";
import { api } from "../lib/api";
import type { Clone } from "../types";

export function ClonesScreen({ clones, onCreated }: { clones: Clone[]; onCreated: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setError("");
    try {
      await api("/api/clones", {
        method: "POST",
        body: JSON.stringify({
          name: form.get("name"),
          handle: form.get("handle") || undefined,
          persona: form.get("persona") || "",
          stylePrompt: form.get("stylePrompt") || "",
          providerConfig: String(form.get("customReferenceId") || "").trim()
            ? { customReferenceId: String(form.get("customReferenceId")) }
            : undefined
        })
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
        <input name="name" placeholder="Name" required />
        <input name="handle" placeholder="Handle" />
        <textarea name="persona" placeholder="Persona" rows={4} />
        <textarea name="stylePrompt" placeholder="Style" rows={4} />
        <input name="customReferenceId" placeholder="Soul ID" />
        {error && <p className="error">{error}</p>}
        <button className="primary" disabled={busy}>
          {busy && <Loader2 className="spin" size={16} />}
          Create clone
        </button>
      </form>
      <section className="clone-list">
        {clones.map((clone) => (
          <article key={clone.id}>
            <h2>{clone.name}</h2>
            <p>@{clone.handle}</p>
          </article>
        ))}
      </section>
    </div>
  );
}
