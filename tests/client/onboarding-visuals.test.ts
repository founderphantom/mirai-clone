import { describe, expect, it } from "vitest";
import {
  canPickMoodboardSelection,
  canSubmitMoodboardSelection,
  nextMoodboardSelection
} from "../../src/client/screens/OnboardingScreen";
import { moodboardVisualFor } from "../../src/client/screens/onboarding-visuals";

describe("onboarding moodboard visuals", () => {
  it("maps curated moodboard slugs to generated moodboard assets", () => {
    const visual = moodboardVisualFor({
      slug: "warm-ambient",
      title: "Warm ambient",
      vibe_summary: "warm ambient creator styling"
    });

    expect(visual).toMatchObject({
      src: "/landing/moodboards/warm-ambient.webp",
      label: "Warm ambient"
    });
  });

  it("slugifies moodboard titles when a slug is not present", () => {
    expect(
      moodboardVisualFor({
        title: "Y2K studio",
        vibe_summary: "glossy studio styling"
      }).src
    ).toBe("/landing/moodboards/y2k-studio.webp");
  });

  it("normalizes special moodboard casing and numerals", () => {
    expect(
      moodboardVisualFor({
        slug: "bw-film",
        title: "BW film",
        vibe_summary: "black and white film stock"
      }).src
    ).toBe("/landing/moodboards/bw-film.webp");

    expect(
      moodboardVisualFor({
        title: "80s horror",
        vibe_summary: "retro horror lighting"
      }).src
    ).toBe("/landing/moodboards/80s-horror.webp");
  });

  it("falls back to the generic moodboard asset for empty input", () => {
    expect(moodboardVisualFor({})).toMatchObject({
      src: "/landing/moodboards/moodboard.webp",
      label: "Moodboard"
    });
  });
});

describe("onboarding moodboard selection rules", () => {
  it("allows submitting one to ten moodboards", () => {
    expect(canSubmitMoodboardSelection(0)).toBe(false);
    expect(canSubmitMoodboardSelection(1)).toBe(true);
    expect(canSubmitMoodboardSelection(5)).toBe(true);
    expect(canSubmitMoodboardSelection(10)).toBe(true);
    expect(canSubmitMoodboardSelection(11)).toBe(false);
  });

  it("allows moodboard picking without checking active clone state", () => {
    expect(canPickMoodboardSelection(0)).toBe(false);
    expect(canPickMoodboardSelection(1)).toBe(true);
  });

  it("allows up to ten selected moodboards", () => {
    const current = Array.from({ length: 9 }, (_, index) => `moodboard_${index}`);

    expect(nextMoodboardSelection(current, "moodboard_9")).toHaveLength(10);
    expect(nextMoodboardSelection([...current, "moodboard_9"], "moodboard_10")).toHaveLength(10);
    expect(nextMoodboardSelection(["moodboard_1"], "moodboard_1")).toEqual([]);
  });
});
