export type FeatureFlags = {
  mobileShell: boolean;
  onboardingInstagram: boolean;
  onboardingStarterSouls: boolean;
  blitzPreview: boolean;
  contextualPaywalls: boolean;
};

const defaults: FeatureFlags = {
  mobileShell: true,
  onboardingInstagram: true,
  onboardingStarterSouls: true,
  blitzPreview: true,
  contextualPaywalls: false
};

let cachedFlags: FeatureFlags = defaults;

export async function loadFlags(): Promise<FeatureFlags> {
  try {
    const response = await fetch("/api/telemetry/config", { credentials: "include" });
    if (!response.ok) return cachedFlags;
    const body = (await response.json()) as { flags?: Partial<FeatureFlags> };
    cachedFlags = { ...defaults, ...(body.flags ?? {}) };
  } catch {
    cachedFlags = defaults;
  }
  return cachedFlags;
}

export function getFlags() {
  return cachedFlags;
}
