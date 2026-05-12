import { describe, expect, it } from "vitest";
import { canAdvanceSwipeDeckAfterAwait } from "../../src/client/components/SwipeDeck";
import { isLoadedBlitzStateForClone } from "../../src/client/screens/BlitzScreen";
import { dailyGenerationMeterValue } from "../../src/client/screens/MeScreen";
import type { BlitzCurrent } from "../../src/client/types";

const blitzCurrent: BlitzCurrent = {
  batch: null,
  usage: {
    imagesToday: 3,
    dailyLimit: 10,
    remaining: 7,
    limitResetsAt: "2026-05-12T00:00:00Z"
  }
};

describe("Blitz client state guards", () => {
  it("only treats loaded Blitz state as current for its clone id", () => {
    expect(isLoadedBlitzStateForClone({ cloneId: "clone-a", data: blitzCurrent }, "clone-a")).toBe(true);
    expect(isLoadedBlitzStateForClone({ cloneId: "clone-a", data: blitzCurrent }, "clone-b")).toBe(false);
    expect(isLoadedBlitzStateForClone(null, "clone-a")).toBe(false);
  });
});

describe("SwipeDeck async advancement guard", () => {
  it("only advances when the swipe resolves for the same deck key", () => {
    expect(canAdvanceSwipeDeckAfterAwait("card-a|card-b", "card-a|card-b")).toBe(true);
    expect(canAdvanceSwipeDeckAfterAwait("card-a|card-b", "card-c|card-d")).toBe(false);
  });
});

describe("account generation usage meter", () => {
  it("uses generationUsage when present, preserving zero values", () => {
    expect(
      dailyGenerationMeterValue(
        {
          imagesToday: 0,
          dailyLimit: 12,
          remaining: 12,
          limitResetsAt: "2026-05-12T00:00:00Z"
        },
        9
      )
    ).toEqual({ value: 0, max: 12 });
  });

  it("falls back to existing generation usage buckets when generationUsage is absent", () => {
    expect(dailyGenerationMeterValue(undefined, 9)).toEqual({ value: 9, max: 24 });
  });
});
