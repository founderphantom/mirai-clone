import { describe, expect, it } from "vitest";
import { extractInstagramCandidateUrls, normalizeInstagramHandle } from "../../src/server/services/instagram-harvest";

describe("Instagram onboarding harvest helpers", () => {
  it("normalizes handles from URLs and @handles", () => {
    expect(normalizeInstagramHandle("@mirai.creator")).toBe("mirai.creator");
    expect(normalizeInstagramHandle("https://www.instagram.com/mirai.creator/?hl=en")).toBe("mirai.creator");
    expect(normalizeInstagramHandle("instagram.com/mirai_creator")).toBe("mirai_creator");
  });

  it("rejects non-profile Instagram routes", () => {
    expect(normalizeInstagramHandle("https://www.instagram.com/reel/abc123")).toBeNull();
    expect(normalizeInstagramHandle("https://www.instagram.com/explore/tags/style")).toBeNull();
  });

  it("extracts likely image URLs from nested post payloads", () => {
    const urls = extractInstagramCandidateUrls({
      items: [
        {
          display_url: "https://scontent.cdninstagram.com/photo-one.jpg",
          permalink: "https://www.instagram.com/p/abc"
        },
        {
          carousel_media: [
            { image_versions2: { candidates: [{ url: "https://instagram.fbcdn.net/photo-two.webp" }] } },
            { thumbnail_url: "https://cdn.example.com/not-ig.html" }
          ]
        }
      ]
    });

    expect(urls).toEqual([
      "https://scontent.cdninstagram.com/photo-one.jpg",
      "https://instagram.fbcdn.net/photo-two.webp"
    ]);
  });
});
