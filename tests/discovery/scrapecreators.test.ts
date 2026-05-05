import { describe, expect, it } from "vitest";
import { normalizeItems } from "../../src/server/discovery/scrapecreators";
import { DISCOVERY_SOURCES } from "../../src/server/discovery/sources";

describe("ScrapeCreators discovery normalization", () => {
  it("maps YouTube Shorts thumbnails into inspiration items", () => {
    const source = DISCOVERY_SOURCES.find((item) => item.id === "youtube-shorts")!;
    const items = normalizeItems(source, {
      shorts: [
        {
          id: "short-1",
          thumbnail: "https://img.youtube.com/vi/short-1/maxresdefault.jpg",
          url: "https://youtube.com/watch?v=short-1",
          title: "Editorial pose",
          viewCountInt: 1200,
          channel: { handle: "studio", title: "Studio" }
        }
      ]
    });

    expect(items[0]).toMatchObject({
      externalId: "short-1",
      platform: "youtube",
      thumbnailUrl: "https://img.youtube.com/vi/short-1/maxresdefault.jpg",
      sourceUrl: "https://youtube.com/watch?v=short-1"
    });
  });

  it("maps TikTok image URL lists when present", () => {
    const source = DISCOVERY_SOURCES.find((item) => item.id === "tiktok-trending")!;
    const items = normalizeItems(source, {
      aweme_list: [
        {
          aweme_id: "tik-1",
          desc: "A strong style reference",
          video: { cover: { url_list: ["https://cdn.example/tik.jpg"] } },
          author: { unique_id: "creator" },
          play_count: 77
        }
      ]
    });

    expect(items[0].imageUrl).toBe("https://cdn.example/tik.jpg");
    expect(items[0].authorHandle).toBe("creator");
  });
});
