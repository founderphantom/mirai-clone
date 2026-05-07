// Sourced from ScrapeCreator research 2026-05 (TikTok + Instagram + Pinterest).
// Re-run research and update when niches go stale.

export interface Niche {
  label: string;
  platforms: string[];
  engagementRank: number;
}

export const NICHES: Niche[] = [
  { label: 'Aesthetic Vibes', platforms: ['instagram', 'tiktok'], engagementRank: 111634231 },
  { label: 'Clean Girl Aesthetic', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 68166893 },
  { label: 'Y2K Fashion', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 42100227 },
  { label: 'Travel Aesthetic', platforms: ['instagram', 'tiktok'], engagementRank: 39646810 },
  { label: 'Dark Academia', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 39415781 },
  { label: 'Cottagecore', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 27532282 },
  { label: 'Quiet Luxury', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 19101361 },
  { label: 'GRWM', platforms: ['instagram', 'tiktok'], engagementRank: 18857812 },
  { label: 'Fashion Aesthetic', platforms: ['instagram', 'pinterest'], engagementRank: 16621141 },
  { label: 'Retro Futurism', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 15247908 },
  { label: 'Barbiecore', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 14527190 },
  { label: 'Old Money', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 10693704 },
  { label: 'Streetwear', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 7151961 },
  { label: 'Dopamine Dressing', platforms: ['instagram', 'pinterest', 'tiktok'], engagementRank: 6706867 },
  { label: 'Tokyo Street Style', platforms: ['instagram', 'pinterest'], engagementRank: 2366963 },
  { label: 'Content Creator', platforms: ['instagram', 'tiktok'], engagementRank: 2111596 },
  { label: 'Skincare Routine', platforms: ['instagram', 'pinterest'], engagementRank: 307892 },
  { label: 'Goblincore', platforms: ['pinterest'], engagementRank: 267857 },
  { label: 'Cherry Blossom Seoul', platforms: ['pinterest'], engagementRank: 214036 },
  { label: 'Bali Vibes', platforms: ['instagram', 'pinterest'], engagementRank: 209155 },
  { label: 'Mob Wife Aesthetic', platforms: ['instagram', 'pinterest'], engagementRank: 205776 },
  { label: 'Preppy Aesthetic', platforms: ['pinterest'], engagementRank: 141623 },
  { label: 'Beauty Routine', platforms: ['instagram'], engagementRank: 94629 },
  { label: 'Coastal Grandmother', platforms: ['instagram', 'pinterest'], engagementRank: 88721 },
  { label: 'Indie Aesthetic', platforms: ['instagram', 'pinterest'], engagementRank: 87398 },
  { label: 'NYC Fashion', platforms: ['pinterest'], engagementRank: 66500 },
  { label: 'Boho Fashion', platforms: ['pinterest'], engagementRank: 61195 },
  { label: 'Dreamcore', platforms: ['pinterest'], engagementRank: 48020 },
  { label: 'Eurotrip Aesthetic', platforms: ['instagram', 'pinterest'], engagementRank: 40639 },
  { label: 'Golden Hour', platforms: ['pinterest'], engagementRank: 31451 },
  { label: 'Winter Minimalist', platforms: ['pinterest'], engagementRank: 31144 },
  { label: 'Vintage Fashion', platforms: ['pinterest'], engagementRank: 28276 },
  { label: 'Angelcore', platforms: ['pinterest'], engagementRank: 25748 },
  { label: 'Moody Aesthetic', platforms: ['tiktok'], engagementRank: 16783 },
  { label: 'OOTD', platforms: ['instagram'], engagementRank: 547 },
  { label: 'Soft Girl', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Photo Dump', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Cinematic Vlog', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Creator Lifestyle', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Vlog Aesthetic', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Coastal Vibes', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Academia Aesthetic', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Mob Wife', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Dark Vibes', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Euphoria Aesthetic', platforms: ['baseline'], engagementRank: 0 },
  { label: 'Desert Wanderlust', platforms: ['baseline'], engagementRank: 0 },
];

const GALLERY_LABELS = [
  'Y2K Fashion',
  'Dark Academia',
  'Cottagecore',
  'Streetwear',
  'Quiet Luxury',
  'Clean Girl Aesthetic',
  'Barbiecore',
  'Vintage Fashion',
  'Preppy Aesthetic',
  'Boho Fashion',
  'Old Money',
  'Coastal Grandmother',
  'Golden Hour',
  'Indie Aesthetic',
  'Eurotrip Aesthetic',
  'Tokyo Street Style',
];

const nicheByLabel = new Map(NICHES.map((niche) => [niche.label, niche]));

// Gallery labels must stay aligned with public/landing/gallery/gallery-*.jpg.
export const GALLERY_NICHES = GALLERY_LABELS.map((label) => nicheByLabel.get(label)).filter(
  (niche): niche is Niche => Boolean(niche)
);

// All labels for the social proof strip marquee
export const PROOF_LABELS = NICHES.map((n) => n.label);
