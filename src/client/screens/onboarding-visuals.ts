export type MoodboardVisualInput = {
  slug?: string;
  title?: string;
  vibe_summary?: string;
};

export type MoodboardVisual = {
  src: string;
  label: string;
};

const MOODBOARD_VISUAL_BASE = "/landing/moodboards";
const FALLBACK_SLUG = "moodboard";

export function moodboardVisualFor(moodboard: MoodboardVisualInput): MoodboardVisual {
  const label = moodboard.title?.trim() || "Moodboard";
  const slug = normalizeSlug(moodboard.slug) || slugify(label) || FALLBACK_SLUG;

  return {
    src: `${MOODBOARD_VISUAL_BASE}/${slug}.webp`,
    label
  };
}

function normalizeSlug(value?: string) {
  return slugify(value ?? "");
}

function slugify(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
