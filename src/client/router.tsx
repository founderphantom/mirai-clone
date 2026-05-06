import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { MobileShell } from "./layout/MobileShell";
import { api } from "./lib/api";
import { identify, track } from "./lib/analytics";
import { loadFlags } from "./lib/flags";
import { AuthScreen } from "./screens/AuthScreen";
import { BlitzScreen } from "./screens/BlitzScreen";
import { ClonesScreen } from "./screens/ClonesScreen";
import { CreateScreen } from "./screens/CreateScreen";
import { InboxScreen } from "./screens/InboxScreen";
import { LandingPage } from "./screens/landing/LandingPage";
import { LibraryScreen } from "./screens/LibraryScreen";
import { MeScreen } from "./screens/MeScreen";
import { OnboardingScreen } from "./screens/OnboardingScreen";
import type { Account, AppData, AppRoute, Clone, Job } from "./types";

function routeFromHash(): AppRoute {
  const value = window.location.hash.replace(/^#\/?/, "");
  if (["blitz", "create", "inbox", "library", "me", "clones", "onboarding"].includes(value)) {
    return value as AppRoute;
  }
  return "blitz";
}

function isAuthHash(): boolean {
  return window.location.hash.replace(/^#\/?/, "") === "auth";
}

export function AppRouter() {
  const [route, setRoute] = useState<AppRoute>(() => routeFromHash());
  const [data, setData] = useState<AppData | null>(null);
  const [authLoading, setAuthLoading] = useState(true);
  const [selectedCloneId, setSelectedCloneId] = useState("");
  const [showAuth, setShowAuth] = useState(() => isAuthHash());

  async function loadAll() {
    const [account, cloneData, jobData] = await Promise.all([
      api<Account>("/api/account"),
      api<{ clones: Clone[] }>("/api/clones"),
      api<{ jobs: Job[] }>("/api/generations")
    ]);
    setData({ account, clones: cloneData.clones, jobs: jobData.jobs });
    setSelectedCloneId((current) => current || cloneData.clones[0]?.id || "");
    identify(account.user.id, { email: account.user.email });
  }

  useEffect(() => {
    loadFlags().catch(() => undefined);
    loadAll()
      .catch(() => setData(null))
      .finally(() => setAuthLoading(false));
  }, []);

  useEffect(() => {
    const onHash = () => {
      setRoute(routeFromHash());
      setShowAuth(isAuthHash());
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const effectiveRoute = useMemo<AppRoute>(() => {
    if (!data) return route;
    if (data.clones.length === 0 && !["me", "onboarding", "clones"].includes(route)) return "onboarding";
    return route;
  }, [data, route]);

  function navigate(next: AppRoute) {
    track("screen_view", { route: next });
    window.location.hash = `/${next}`;
    setRoute(next);
  }

  function openAuth() {
    window.location.hash = "auth";
    setShowAuth(true);
  }

  function closeAuth() {
    window.location.hash = "";
    setShowAuth(false);
  }

  if (authLoading) {
    return (
      <main className="auth-screen">
        <Loader2 className="spin" />
      </main>
    );
  }

  if (!data) {
    return (
      <>
        <LandingPage onGetStarted={openAuth} />
        {showAuth && (
          <div
            onClick={closeAuth}
            style={{ position: "fixed", inset: 0, zIndex: 200, background: "rgba(0,0,0,0.55)", display: "flex", alignItems: "center", justifyContent: "center" }}
          >
            <div onClick={(e) => e.stopPropagation()} style={{ width: "100%", maxWidth: 460, padding: "0 16px" }}>
              <AuthScreen onDone={() => { closeAuth(); loadAll(); }} />
            </div>
          </div>
        )}
      </>
    );
  }

  return (
    <MobileShell
      account={data.account}
      clones={data.clones}
      route={effectiveRoute}
      selectedCloneId={selectedCloneId}
      onRoute={navigate}
      onClone={setSelectedCloneId}
      onSignedOut={() => setData(null)}
    >
      {effectiveRoute === "onboarding" && <OnboardingScreen onCreated={loadAll} />}
      {effectiveRoute === "blitz" && <BlitzScreen jobs={data.jobs} />}
      {effectiveRoute === "create" && (
        <CreateScreen clones={data.clones} selectedCloneId={selectedCloneId} onSubmitted={loadAll} />
      )}
      {effectiveRoute === "inbox" && <InboxScreen jobs={data.jobs} />}
      {effectiveRoute === "library" && <LibraryScreen jobs={data.jobs} onRefresh={loadAll} />}
      {effectiveRoute === "me" && <MeScreen account={data.account} />}
      {effectiveRoute === "clones" && <ClonesScreen clones={data.clones} onCreated={loadAll} />}
    </MobileShell>
  );
}
