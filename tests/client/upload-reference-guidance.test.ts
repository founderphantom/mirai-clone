import { describe, expect, it } from "vitest";
import { validateReferenceFiles } from "../../src/client/screens/onboarding/upload-reference-guidance";

function file(name: string, type = "image/jpeg", size = 1024) {
  return new File(["x".repeat(size)], name, { type });
}

describe("upload reference guidance validation", () => {
  it("requires at least 5 images", () => {
    expect(validateReferenceFiles([file("one.jpg"), file("two.jpg"), file("three.jpg"), file("four.jpg")])).toMatchObject({
      valid: false,
      message: "Upload at least 5 reference photos."
    });
  });

  it("accepts 5 to 15 image files up to 15 MB each", () => {
    expect(validateReferenceFiles(Array.from({ length: 5 }, (_, index) => file(`${index}.jpg`)))).toMatchObject({
      valid: true,
      message: "5 photos ready."
    });
  });

  it("rejects more than 15 files", () => {
    expect(validateReferenceFiles(Array.from({ length: 16 }, (_, index) => file(`${index}.jpg`)))).toMatchObject({
      valid: false,
      message: "Upload no more than 15 reference photos."
    });
  });

  it("rejects non-image files", () => {
    expect(validateReferenceFiles([file("one.jpg"), file("two.jpg"), file("three.jpg"), file("four.jpg"), file("notes.txt", "text/plain")])).toMatchObject({
      valid: false,
      message: "Reference uploads must be image files."
    });
  });

  it("rejects images larger than 15 MB", () => {
    expect(validateReferenceFiles([file("one.jpg"), file("two.jpg"), file("three.jpg"), file("four.jpg"), file("huge.jpg", "image/jpeg", 15 * 1024 * 1024 + 1)])).toMatchObject({
      valid: false,
      message: "Each reference photo must be 15 MB or smaller."
    });
  });
});
