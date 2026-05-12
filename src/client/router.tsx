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

type AuthMode = "signin" | "signup";

const APP_ROUTES: AppRoute[] = ["blitz", "create", "inbox", "library", "me", "clones", "onboarding"];

function routeFromLocation(): AppRoute {
  const hashValue = window.location.hash.replace(/^#\/?/, "");
  if (APP_ROUTES.includes(hashValue as AppRoute)) {
    return hashValue as AppRoute;
  }

  const pathValue = window.location.pathname.replace(/^\/+/, "").replace(/\/$/, "");
  if (APP_ROUTES.includes(pathValue as AppRoute)) {
    return pathValue as AppRoute;
  }

  const legacyValue = hashValue === "auth" ? "" : hashValue;
  if (APP_ROUTES.includes(legacyValue as AppRoute)) {
    return legacyValue as AppRoute;
  }

  return "blitz";
}

function appPath(route: AppRoute): string {
  return `/${route}`;
}

function normalizedPath(): string {
  return window.location.pathname.replace(/^\/+/, "").replace(/\/$/, "");
}

function isAppRoutePath() {
  const pathValue = normalizedPath();
  return APP_ROUTES.includes(pathValue as AppRoute);
}

function authModeFromLocation(): AuthMode | null {
  const pathValue = normalizedPath();
  if (pathValue === "signup") return "signup";
  if (pathValue === "login") return "signin";
  if (window.location.hash.replace(/^#\/?/, "") === "auth") return "signup";
  return null;
}

export function AppRouter() {
  const [route, setRoute] = useState<AppRoute>(() => routeFromLocation());
  const [data, setData] = useState<AppData | null>(null);
  const [authLoading, setAuthLoading] = useState(true);
  const [selectedCloneId, setSelectedCloneId] = useState("");
  const [authMode, setAuthMode] = useState<AuthMode | null>(() => authModeFromLocation());

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
    const onLocation = () => {
      setRoute(routeFromLocation());
      setAuthMode(authModeFromLocation());
    };
    window.addEventListener("hashchange", onLocation);
    window.addEventListener("popstate", onLocation);
    return () => {
      window.removeEventListener("hashchange", onLocation);
      window.removeEventListener("popstate", onLocation);
    };
  }, []);

  useEffect(() => {
    if (!data || !authMode) return;
    const nextRoute = data.clones.length === 0 ? "onboarding" : "blitz";
    window.history.replaceState(null, "", appPath(nextRoute));
    setRoute(nextRoute);
    setAuthMode(null);
  }, [authMode, data]);

  const effectiveRoute = useMemo<AppRoute>(() => {
    if (!data) return route;
    if (data.clones.length === 0 && !["me", "onboarding", "clones"].includes(route)) return "onboarding";
    return route;
  }, [data, route]);

  function navigate(next: AppRoute) {
    track("screen_view", { route: next });
    window.history.pushState(null, "", appPath(next));
    setRoute(next);
  }

  function openAuth() {
    window.history.pushState(null, "", "/signup");
    setAuthMode("signup");
  }

  function changeAuthMode(next: AuthMode) {
    window.history.pushState(null, "", next === "signup" ? "/signup" : "/login");
    setAuthMode(next);
  }

  async function completeAuth() {
    setAuthLoading(true);
    window.history.replaceState(null, "", appPath("onboarding"));
    setRoute("onboarding");
    try {
      await loadAll();
      setAuthMode(null);
    } finally {
      setAuthLoading(false);
    }
  }

  if (authLoading) {
    return (
      <main className="auth-screen">
        <Loader2 className="spin" />
      </main>
    );
  }

  if (!data) {
    if (isAppRoutePath()) {
      return <AuthScreen initialMode="signin" onDone={completeAuth} onModeChange={changeAuthMode} />;
    }

    if (authMode) {
      return <AuthScreen initialMode={authMode} onDone={completeAuth} onModeChange={changeAuthMode} />;
    }

    return (
      <LandingPage onGetStarted={openAuth} />
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
      {effectiveRoute === "blitz" && <BlitzScreen clones={data.clones} selectedCloneId={selectedCloneId} />}
      {effectiveRoute === "create" && (
        <CreateScreen clones={data.clones} selectedCloneId={selectedCloneId} onSubmitted={loadAll} />
      )}
      {effectiveRoute === "inbox" && <InboxScreen jobs={data.jobs} />}
      {effectiveRoute === "library" && <LibraryScreen jobs={data.jobs} onRefresh={loadAll} />}
      {effectiveRoute === "me" && <MeScreen account={data.account} onSignedOut={() => setData(null)} />}
      {effectiveRoute === "clones" && <ClonesScreen clones={data.clones} onCreated={loadAll} />}
    </MobileShell>
  );
}
