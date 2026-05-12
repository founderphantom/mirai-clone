import { ArrowRight, Loader2, Sparkles } from "lucide-react";
import { type CSSProperties, type FormEvent, useEffect, useId, useState } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";

type AuthMode = "signin" | "signup";

type AuthScreenProps = {
  initialMode?: AuthMode;
  onDone: () => void | Promise<void>;
  onModeChange?: (mode: AuthMode) => void;
};

const HERO_IMAGES = [
  "/landing/clone-y2k-cafe.jpg",
  "/landing/clone-tokyo-neon.jpg",
  "/landing/step-moodboards.jpg"
];

export function AuthScreen({ initialMode = "signup", onDone, onModeChange }: AuthScreenProps) {
  const [mode, setMode] = useState<AuthMode>(initialMode);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const titleId = useId();

  useEffect(() => {
    setMode(initialMode);
    setError("");
  }, [initialMode]);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const errors = params.getAll("error");
    if (errors.length === 0) return;

    setError(
      errors.includes("google")
        ? "Google sign-in did not complete. Please try again."
        : "Sign-in did not complete. Please try again."
    );
    window.history.replaceState(null, "", window.location.pathname);
  }, []);

  function switchMode(next: AuthMode) {
    setMode(next);
    setError("");
    onModeChange?.(next);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const email = String(form.get("email") || "");
    const password = String(form.get("password") || "");
    const name = String(form.get("name") || "Mirai creator");
    setBusy(true);
    setError("");
    track(mode === "signup" ? "signup_submit" : "signin_submit");

    try {
      const result =
        mode === "signup"
          ? await authClient.signUp.email({ email, password, name })
          : await authClient.signIn.email({ email, password });

      if (result.error) {
        setError(result.error.message || "Authentication failed.");
        return;
      }

      await onDone();
    } finally {
      setBusy(false);
    }
  }

  async function continueWithGoogle() {
    setBusy(true);
    setError("");
    track(mode === "signup" ? "signup_google_click" : "signin_google_click");

    try {
      const result = await authClient.signIn.social({
        provider: "google",
        callbackURL: "/onboarding",
        newUserCallbackURL: "/onboarding",
        errorCallbackURL: mode === "signup" ? "/signup?error=google" : "/login?error=google"
      });

      if (result?.error) {
        setError(result.error.message || "Google sign-in failed.");
      }
    } catch {
      setError("Google sign-in could not start. Check your OAuth configuration.");
    } finally {
      setBusy(false);
    }
  }

  const isSignup = mode === "signup";

  return (
    <main className="auth-page" aria-labelledby={titleId}>
      <div className="auth-page__texture" aria-hidden="true" />
      <section className="auth-page__inner">
        <aside className="auth-story" aria-label="Mirai product preview">
          <a className="auth-brand" href="/">
            <img src="/icons/mirai-icon.svg" alt="" width={34} height={34} />
            <span>Mirai</span>
          </a>
          <div className="auth-copy">
            <span className="auth-kicker">
              <Sparkles size={15} />
              Soul-first creator studio
            </span>
            <h1 id={titleId}>{isSignup ? "Create your Mirai account" : "Welcome back to Mirai"}</h1>
            <p>
              {isSignup
                ? "Start with Instagram, manual uploads, or a preset Soul, then pick your first inspiration moodboards."
                : "Jump back into Blitz, create new looks, and keep building your creator library."}
            </p>
          </div>
          <div className="auth-preview-stack" aria-hidden="true">
            {HERO_IMAGES.map((src, index) => (
              <img key={src} src={src} alt="" style={{ "--auth-card-index": index } as CSSProperties} />
            ))}
            <div className="auth-preview-caption">
              <strong>First Blitz in progress</strong>
              <span>5 cards seeded from your Soul and moodboard pool</span>
            </div>
          </div>
        </aside>

        <section className="auth-card">
          <div className="auth-card__header">
            <span>{isSignup ? "Get started free" : "Log in"}</span>
            <h2>{isSignup ? "Welcome to Mirai" : "Welcome back"}</h2>
            <p>{isSignup ? "No blank prompts. Your Soul setup starts right after signup." : "Use the same account you used to create your Soul."}</p>
          </div>

          <div className="auth-mode-toggle" aria-label="Authentication mode">
            <button type="button" className={isSignup ? "active" : ""} onClick={() => switchMode("signup")}>
              Sign up
            </button>
            <button type="button" className={!isSignup ? "active" : ""} onClick={() => switchMode("signin")}>
              Log in
            </button>
          </div>

          <button className="auth-google-button" type="button" onClick={continueWithGoogle} disabled={busy}>
            <svg className="auth-google-icon" aria-hidden="true" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="20" height="20">
              <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
              <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
              <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l3.66-2.84z"/>
              <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
            </svg>
            Continue with Google
          </button>

          <div className="auth-divider"><span>or continue with email</span></div>

          <form className="auth-form" onSubmit={submit}>
            {isSignup && (
              <label>
                <span>Name</span>
                <input name="name" autoComplete="name" placeholder="Jamaal" />
              </label>
            )}
            <label>
              <span>Email</span>
              <input name="email" type="email" autoComplete="email" placeholder="you@example.com" required />
            </label>
            <label>
              <span>Password</span>
              <input
                name="password"
                type="password"
                autoComplete={isSignup ? "new-password" : "current-password"}
                placeholder="At least 8 characters"
                minLength={8}
                required
              />
            </label>
            {error && <p className="auth-error" role="alert">{error}</p>}
            <button className="auth-submit" disabled={busy}>
              {busy && <Loader2 className="spin" size={17} />}
              {isSignup ? "Create account" : "Log in"}
              {!busy && <ArrowRight size={17} />}
            </button>
          </form>

          <div className="auth-callout">
            <div>
              <strong>{isSignup ? "Soul Character creation pending script" : "Your daily Blitz is waiting"}</strong>
              <span>
                {isSignup
                  ? "For now, Mirai collects the eligible Instagram/upload/preset inputs and parks them for your Soul script."
                  : "Saved cards, clone profiles, and moodboard taste signals stay attached to your account."}
              </span>
            </div>
          </div>

          <p className="auth-switch">
            {isSignup ? "Already have an account?" : "New to Mirai?"}{" "}
            <button type="button" onClick={() => switchMode(isSignup ? "signin" : "signup")}>
              {isSignup ? "Log in" : "Create one"}
            </button>
          </p>
        </section>
      </section>
    </main>
  );
}
