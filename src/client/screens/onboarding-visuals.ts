export type BubbleVisualInput = {
  slug?: string;
  title?: string;
  vibe_summary?: string;
};

export type BubbleVisual = {
  src: string;
  label: string;
};

type BubbleVisualRule = BubbleVisual & {
  terms: string[];
};

const BUBBLE_VISUAL_BASE = "/landing/onboarding-bubbles";

const BUBBLE_VISUAL_RULES: BubbleVisualRule[] = [
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-beauty.png`,
    label: "Beauty",
    terms: ["beauty", "skincare", "skin", "makeup", "glam", "glow", "dewy", "grwm", "fragrance", "spa"]
  },
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-travel.png`,
    label: "Travel",
    terms: ["travel", "coastal", "resort", "hotel", "balcony", "ocean", "beach", "airport", "vacation", "passport", "suitcase", "desert", "palm"]
  },
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-wellness.png`,
    label: "Wellness",
    terms: ["wellness", "fitness", "pilates", "yoga", "gym", "matcha", "workout", "athleisure", "ritual"]
  },
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-fashion.png`,
    label: "Fashion",
    terms: ["fashion", "style", "streetwear", "outfit", "editorial", "runway", "wardrobe", "luxe", "jewelry", "leather", "denim", "fit check"]
  },
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-vibes.png`,
    label: "Vibes",
    terms: ["vibe", "vibes", "y2k", "retro", "neon", "night", "cinematic", "cafe", "coffee", "festival", "moody", "bokeh"]
  },
  {
    src: `${BUBBLE_VISUAL_BASE}/bubble-content.png`,
    label: "Content",
    terms: ["content", "creator", "camera", "ring light", "studio", "youtube", "tiktok", "instagram"]
  }
];

const FALLBACK_VISUAL = BUBBLE_VISUAL_RULES.find((rule) => rule.label === "Vibes") ?? BUBBLE_VISUAL_RULES[BUBBLE_VISUAL_RULES.length - 1];

export function bubbleVisualFor(bubble: BubbleVisualInput): BubbleVisual {
  const searchable = [bubble.slug, bubble.title, bubble.vibe_summary]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();

  const match = BUBBLE_VISUAL_RULES.find((rule) =>
    rule.terms.some((term) => hasSearchTerm(searchable, term))
  ) ?? FALLBACK_VISUAL;

  return {
    src: match.src,
    label: match.label
  };
}

function hasSearchTerm(searchable: string, term: string) {
  if (term.includes(" ")) return searchable.includes(term);
  return new RegExp(`\\b${escapeRegExp(term)}\\b`).test(searchable);
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
