import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { AppRouter } from "./router";
import { initAnalytics } from "./lib/analytics";
import { initErrorTracking } from "./lib/sentry";
import "./styles.css";
import "./mobile.css";
import "./app-ui.css";
import "./auth.css";
import "./onboarding.css";
import "./onboarding-upload.css";

initErrorTracking();
initAnalytics();

if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => undefined);
  });
}

const useLegacyShell = new URLSearchParams(window.location.search).has("legacy");

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {useLegacyShell ? <App /> : <AppRouter />}
  </React.StrictMode>
);
