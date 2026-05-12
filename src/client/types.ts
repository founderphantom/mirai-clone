export type Clone = {
  id: string;
  display_name: string;
  handle: string;
  source?: "manual_upload" | "starter" | "future_instagram";
  status?: "active" | "archived" | "deleting";
  soul_status?: "draft" | "queued" | "training" | "ready" | "failed" | "provider_action_required";
  provider?: "higgsfield";
  provider_soul_id?: string | null;
  reference_count_total?: number;
  reference_count_training_selected?: number;
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
  prompt?: string | null;
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

export type Moodboard = {
  id: string;
  slug: string;
  title: string;
  vibe_summary: string;
  searchQueries: string[];
  selected: boolean;
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

export type BlitzImage = {
  outputId: string;
  mediaUrl: string;
  visualReferenceId: string | null;
  swipeIndex: number;
  swiped: boolean;
};

export type BlitzBatch = {
  id: string;
  batchNumber: number;
  status: string;
  images: BlitzImage[];
};

export type GenerationUsage = {
  imagesToday: number;
  dailyLimit: number;
  remaining: number;
  limitResetsAt: string;
};

export type BlitzCurrent = {
  batch: BlitzBatch | null;
  status?: string | null;
  progress?: { phase: string; detail: string } | null;
  usage: GenerationUsage;
  nextBatchStatus?: string | null;
};

export type OnboardingState = {
  clones: Clone[];
  activeClone: Clone | null;
  latestHarvest?: InstagramHarvestJob | null;
  moodboards: Moodboard[];
  inspirationPoolCount: number;
  starters: StarterCharacter[];
  instagram?: { enabled: boolean; status: string };
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
