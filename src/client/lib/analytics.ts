type AnalyticsProps = Record<string, unknown>;

const queue: Array<{ event: string; props: AnalyticsProps }> = [];
let identifiedUserId: string | null = null;
let enabled = true;

export function initAnalytics() {
  if (typeof window === "undefined") return;
  enabled = !window.location.hostname.includes("localhost") || window.localStorage.getItem("mirai.analytics.dev") === "1";
  track("app_open", {
    path: window.location.pathname,
    legacy: new URLSearchParams(window.location.search).has("legacy")
  });
  void flush();
}

export function identify(userId: string, props: AnalyticsProps = {}) {
  identifiedUserId = userId;
  track("identify", props);
}

export function track(event: string, props: AnalyticsProps = {}) {
  if (!enabled) return;
  queue.push({ event, props: { ...props, userId: identifiedUserId ?? undefined } });
  if (queue.length >= 4) void flush();
}

export async function flush() {
  if (!enabled || queue.length === 0) return;
  const next = queue.splice(0, queue.length);
  try {
    await fetch("/api/telemetry/events", {
      method: "POST",
      credentials: "include",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ events: next })
    });
  } catch {
    queue.unshift(...next.slice(-10));
  }
}
