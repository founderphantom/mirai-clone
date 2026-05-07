// Sourced from ScrapeCreator research 2026-05 (TikTok + Instagram + Pinterest).
// Re-run research and update when niches go stale.

export interface Niche {
  label: string;
  platforms: string[];
  engagementRank: number;
}

export const NICHES: Niche[] = [
  { label: 'Y2K Fashion', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Dark Academia', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Cottagecore', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Streetwear', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Quiet Luxury', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Mob Wife Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Clean Girl Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Barbiecore', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Vintage Fashion', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Preppy Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Boho Fashion', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Old Money', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Beauty Routine', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Skincare Routine', platforms: ['baseline'], engagementRank: 1 },
  { label: 'GRWM', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Soft Girl', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Aesthetic Vibes', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Coastal Grandmother', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Golden Hour', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Moody Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Indie Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Retro Futurism', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Dreamcore', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Goblincore', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Angelcore', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Travel Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Bali Vibes', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Tokyo Street Style', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Eurotrip Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Cherry Blossom Seoul', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Winter Minimalist', platforms: ['baseline'], engagementRank: 1 },
  { label: 'NYC Fashion', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Content Creator', platforms: ['baseline'], engagementRank: 1 },
  { label: 'OOTD', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Fashion Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Photo Dump', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Cinematic Vlog', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Creator Lifestyle', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Vlog Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Coastal Vibes', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Academia Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Dopamine Dressing', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Mob Wife', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Dark Vibes', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Euphoria Aesthetic', platforms: ['baseline'], engagementRank: 1 },
  { label: 'Desert Wanderlust', platforms: ['baseline'], engagementRank: 1 },
];

// Top 16 niches for the gallery strip
export const GALLERY_NICHES = NICHES.slice(0, 16);

// All labels for the social proof strip marquee
export const PROOF_LABELS = NICHES.map((n) => n.label);
