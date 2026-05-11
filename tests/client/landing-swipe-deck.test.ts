import { describe, expect, it } from "vitest";
import {
  getDeckCardMotionState,
  getNextDeckIndex,
  resolveSwipeDirection
} from "../../src/client/screens/landing/SwipeDeckHero";

describe("landing swipe deck animation helpers", () => {
  it("keeps stable stacked card targets for smooth restacking", () => {
    expect(getDeckCardMotionState(0, 7)).toMatchObject({
      x: 0,
      y: 0,
      rotate: 0,
      scale: 1,
      opacity: 1,
      zIndex: 7
    });

    expect(getDeckCardMotionState(1, 7)).toMatchObject({
      x: 12,
      y: 10,
      rotate: -2.5,
      scale: 0.965,
      opacity: 1,
      zIndex: 6
    });

    expect(getDeckCardMotionState(4, 7).opacity).toBe(0);
  });

  it("resolves swipe direction from deliberate offset or velocity", () => {
    expect(resolveSwipeDirection(91, 0)).toBe(1);
    expect(resolveSwipeDirection(-91, 0)).toBe(-1);
    expect(resolveSwipeDirection(0, 501)).toBe(1);
    expect(resolveSwipeDirection(0, -501)).toBe(-1);
    expect(resolveSwipeDirection(40, 120)).toBe(0);
  });

  it("advances through the same deck order for left and right swipes", () => {
    expect(getNextDeckIndex(0, -1, 7)).toBe(1);
    expect(getNextDeckIndex(0, 1, 7)).toBe(1);
    expect(getNextDeckIndex(6, -1, 7)).toBe(0);

    const mixedSwipeIndexes = [-1, 1, -1, 1].reduce<number[]>(
      (indexes, direction) => [
        ...indexes,
        getNextDeckIndex(indexes[indexes.length - 1], direction as -1 | 1, 7)
      ],
      [0]
    );

    expect(mixedSwipeIndexes).toEqual([0, 1, 2, 3, 4]);
  });
});
