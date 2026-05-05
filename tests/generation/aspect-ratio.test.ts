import { describe, expect, it } from "vitest";
import { closestAspectRatio, dimensionsForQuality } from "../../src/server/generation/aspect-ratio";

describe("aspect ratio helpers", () => {
  it("matches portrait inspiration to 3:4", () => {
    expect(closestAspectRatio(1536, 2048)).toBe("3:4");
  });

  it("falls back to the current Higgsfield default", () => {
    expect(closestAspectRatio()).toBe("3:4");
  });

  it("keeps the captured Soul v2 quality dimensions", () => {
    expect(dimensionsForQuality("1080p")).toEqual({ width: 1536, height: 2048 });
    expect(dimensionsForQuality("2K")).toEqual({ width: 2048, height: 2732 });
  });
});
