interface CloudflareBindings {
  ASSETS: Fetcher;
  DB: D1Database;
  MEDIA: R2Bucket;
  GENERATION_QUEUE: Queue<import("./src/server/queue/messages").GenerationQueueMessage>;
  ONBOARDING_QUEUE: Queue<import("./src/server/queue/messages").OnboardingQueueMessage>;
  APP_NAME: string;
  APP_URL: string;
  BETTER_AUTH_SECRET?: string;
  POLAR_ACCESS_TOKEN?: string;
  POLAR_WEBHOOK_SECRET?: string;
  POLAR_PRO_PRODUCT_ID?: string;
  POLAR_SERVER?: "sandbox" | "production";
  SCRAPECREATORS_API_KEY?: string;
  SCRAPECREATORS_CACHE_TTL_SECONDS?: string;
  SCRAPECREATORS_TIKTOK_TRENDING_ENDPOINT?: string;
  SCRAPECREATORS_INSTAGRAM_REELS_ENDPOINT?: string;
  SCRAPECREATORS_INSTAGRAM_PROFILE_ENDPOINT?: string;
  SCRAPECREATORS_INSTAGRAM_POSTS_ENDPOINT?: string;
  DISCOVERY_DEFAULT_REGION?: string;
  ANTHROPIC_API_KEY?: string;
  ANTHROPIC_PERSONA_MODEL?: string;
  HIGGSFIELD_JWT?: string;
  HIGGSFIELD_SESSION_ID?: string;
  HIGGSFIELD_CLIENT_COOKIE?: string;
  HIGGSFIELD_DEFAULT_CHARACTER_ID?: string;
  HIGGSFIELD_DEFAULT_STYLE_ID?: string;
}
