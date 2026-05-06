import { Loader2 } from "lucide-react";
import { FormEvent, useState } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";

export function AuthScreen({ onDone }: { onDone: () => void }) {
  const [mode, setMode] = useState<"signin" | "signup">("signup");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const email = String(form.get("email") || "");
    const password = String(form.get("password") || "");
    const name = String(form.get("name") || "Mirai creator");
    setBusy(true);
    setError("");
    track(mode === "signup" ? "signup_submit" : "signin_submit");
    const result =
      mode === "signup"
        ? await authClient.signUp.email({ email, password, name })
        : await authClient.signIn.email({ email, password });
    setBusy(false);
    if (result.error) {
      setError(result.error.message || "Authentication failed.");
      return;
    }
    onDone();
  }

  return (
    <main className="auth-screen mobile-auth">
      <section className="auth-panel">
        <div className="brand-mark">Mirai</div>
        <div className="segmented">
          <button className={mode === "signup" ? "active" : ""} onClick={() => setMode("signup")}>Sign up</button>
          <button className={mode === "signin" ? "active" : ""} onClick={() => setMode("signin")}>Sign in</button>
        </div>
        <form onSubmit={submit}>
          {mode === "signup" && <input name="name" placeholder="Name" />}
          <input name="email" type="email" placeholder="Email" required />
          <input name="password" type="password" placeholder="Password" minLength={8} required />
          {error && <p className="error">{error}</p>}
          <button className="primary" disabled={busy}>
            {busy && <Loader2 className="spin" size={16} />}
            {mode === "signup" ? "Create account" : "Sign in"}
          </button>
        </form>
      </section>
    </main>
  );
}
