import { describe, expect, it } from "vitest";
import { bubbleVisualFor } from "../../src/client/screens/onboarding-visuals";

describe("onboarding bubble visuals", () => {
  it("maps beauty and skincare bubbles to the beauty asset", () => {
    const visual = bubbleVisualFor({
      slug: "dewy-skin",
      title: "Dewy Skin",
      vibe_summary: "skincare, makeup, soft glow"
    });

    expect(visual).toMatchObject({
      src: "/landing/onboarding-bubbles/bubble-beauty.png",
      label: "Beauty"
    });
  });

  it("maps travel bubbles to the travel asset", () => {
    expect(
      bubbleVisualFor({
        slug: "coastal-escape",
        title: "Coastal Escape",
        vibe_summary: "hotel balcony, ocean views, suitcase"
      }).src
    ).toBe("/landing/onboarding-bubbles/bubble-travel.png");
  });

  it("maps creator and content bubbles to the content asset", () => {
    expect(
      bubbleVisualFor({
        slug: "creator-kit",
        title: "Creator Kit",
        vibe_summary: "camera, ring light, social content"
      }).src
    ).toBe("/landing/onboarding-bubbles/bubble-content.png");
  });

  it("prefers the vibes asset for neon nightlife bubbles", () => {
    expect(
      bubbleVisualFor({
        slug: "tokyo-neon",
        title: "Tokyo Neon",
        vibe_summary: "Night city color, glossy styling, and bright social hooks"
      }).src
    ).toBe("/landing/onboarding-bubbles/bubble-vibes.png");
  });

  it("prefers the wellness asset for Pilates bubbles even when the summary mentions content", () => {
    expect(
      bubbleVisualFor({
        slug: "pilates-morning",
        title: "Pilates Morning",
        vibe_summary: "Wellness studio energy, activewear sets, and calm routine content"
      }).src
    ).toBe("/landing/onboarding-bubbles/bubble-wellness.png");
  });

  it("maps fashion and streetwear bubbles to the fashion asset", () => {
    expect(
      bubbleVisualFor({
        slug: "street-style",
        title: "Street Style",
        vibe_summary: "fashion, outfit checks, editorial styling"
      }).src
    ).toBe("/landing/onboarding-bubbles/bubble-fashion.png");
  });

  it("falls back to the vibes asset for unknown aesthetics", () => {
    expect(
      bubbleVisualFor({
        slug: "abstract",
        title: "Abstract",
        vibe_summary: "experimental visual language"
      })
    ).toMatchObject({
      src: "/landing/onboarding-bubbles/bubble-vibes.png",
      label: "Vibes"
    });
  });
});
