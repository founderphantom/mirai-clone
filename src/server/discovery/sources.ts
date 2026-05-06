export type DiscoverySourceId = "youtube-shorts" | "tiktok-trending" | "instagram-reels";

export type DiscoverySourceConfig = {
  id: DiscoverySourceId;
  label: string;
  platform: string;
  defaultEndpoint: string;
  defaultParams: Record<string, string>;
  notes: string;
};

export const DISCOVERY_SOURCES: DiscoverySourceConfig[] = [
  {
    id: "youtube-shorts",
    label: "YouTube Shorts",
    platform: "youtube",
    defaultEndpoint: "/v1/youtube/shorts/trending",
    defaultParams: {},
    notes: "Documented ScrapeCreators trending endpoint with thumbnail URLs."
  },
  {
    id: "tiktok-trending",
    label: "TikTok Trending",
    platform: "tiktok",
    defaultEndpoint: "/v1/tiktok/trending/feed",
    defaultParams: { region: "US", trim: "true" },
    notes: "Kept configurable because ScrapeCreators exposes Trending Feed but docs can move slugs."
  },
  {
    id: "instagram-reels",
    label: "Instagram Reels",
    platform: "instagram",
    defaultEndpoint: "/v2/instagram/reels/search",
    defaultParams: { query: "fashion editorial", trim: "true" },
    notes: "Search-based source until an explicit trending Reels endpoint is enabled."
  }
];

export function getDiscoverySource(id: string): DiscoverySourceConfig | undefined {
  return DISCOVERY_SOURCES.find((source) => source.id === id);
}
