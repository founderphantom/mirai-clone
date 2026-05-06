import { track } from "./analytics";

export function initErrorTracking() {
  if (typeof window === "undefined") return;
  window.addEventListener("error", (event) => {
    track("client_error", {
      message: event.message,
      source: event.filename,
      line: event.lineno,
      column: event.colno
    });
  });
  window.addEventListener("unhandledrejection", (event) => {
    track("client_unhandled_rejection", {
      message: event.reason instanceof Error ? event.reason.message : String(event.reason)
    });
  });
}
