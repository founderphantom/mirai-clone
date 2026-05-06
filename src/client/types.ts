export type Clone = {
  id: string;
  name: string;
  handle: string;
  persona: string;
  style_prompt: string;
  soul_source?: "instagram" | "manual_upload" | "starter" | "manual";
  soul_status?: "pending_script" | "ready" | "failed";
  starter_character_id?: string | null;
  provider_config_json?: string;
  reference_count?: number;
  generation_count?: number;
};

export type DiscoveryItem = {
  id: string;
  title: string;
  platform: string;
  image_url: string | null;
  thumbnail_url: string | null;
  source_url: string | null;
  author_handle: string;
};

export type Job = {
  id: string;
  clone_id: string;
  clone_name?: string;
  status: string;
  mode?: "image" | "video";
  prompt: string;
  updated_at: string;
  output_count?: number;
  preview_media_id?: string | null;
};

export type Account = {
  user: { id: string; name?: string; email?: string };
  billing: {
    checkoutEnabled: boolean;
    portalEnabled: boolean;
    polarEnabled?: boolean;
    server: string;
    recentEvents?: Array<{
      event_type: string;
      polar_product_id: string | null;
      created_at: string;
    }>;
  };
};

export type Inspiration =
  | { type: "discovery"; id: string; imageUrl: string }
  | { type: "asset"; id: string; imageUrl: string };

export type StarterCharacter = {
  id: string;
  slug: string;
  name: string;
  persona: string;
  style_prompt: string;
  hero_media_id: string | null;
  sort?: number;
  status: string;
};

export type InspirationBubble = {
  id: string;
  slug: string;
  title: string;
  vibe_summary: string;
  searchQueries: string[];
  selected: number;
};

export type InstagramHarvestJob = {
  id: string;
  handle: string;
  status: string;
  candidate_count: number;
  accepted_count: number;
  fail_reason: string | null;
  clone_id: string | null;
};

export type OnboardingState = {
  clones: Clone[];
  activeClone: Clone | null;
  latestHarvest: InstagramHarvestJob | null;
  bubbles: InspirationBubble[];
  inspirationPoolCount: number;
  starters: StarterCharacter[];
};

export type AppRoute =
  | "blitz"
  | "create"
  | "inbox"
  | "library"
  | "me"
  | "clones"
  | "onboarding";

export type AppData = {
  account: Account;
  clones: Clone[];
  jobs: Job[];
};
