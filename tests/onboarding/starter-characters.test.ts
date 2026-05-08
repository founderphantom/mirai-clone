import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { STARTER_CHARACTER_TEMPLATES } from "../../src/server/services/starter-characters";

const expectedFashionPresets = [
  {
    id: "starter_rowan_parsons_crit",
    slug: "rowan-parsons-crit",
    name: "Rowan - Parsons Crit",
    image: "/landing/starters/starter-rowan.jpg"
  },
  {
    id: "starter_liora_downtown_muse",
    slug: "liora-downtown-muse",
    name: "Liora - Downtown Muse",
    image: "/landing/starters/starter-liora.jpg"
  },
  {
    id: "starter_dorian_experimental_dresser",
    slug: "dorian-experimental-dresser",
    name: "Dorian - Experimental Dresser",
    image: "/landing/starters/starter-dorian.jpg"
  },
  {
    id: "starter_sable_form_study",
    slug: "sable-form-study",
    name: "Sable - Form Study",
    image: "/landing/starters/starter-sable.jpg"
  },
  {
    id: "starter_vesper_goth_rap_streetwear",
    slug: "vesper-goth-rap-streetwear",
    name: "Vesper - Goth Rap Streetwear",
    image: "/landing/starters/starter-vesper.jpg"
  },
  {
    id: "starter_niko_underground_set",
    slug: "niko-underground-set",
    name: "Niko - Underground Set",
    image: "/landing/starters/starter-niko.jpg"
  },
  {
    id: "starter_ren_niche_designer",
    slug: "ren-niche-designer",
    name: "Ren - Niche Designer",
    image: "/landing/starters/starter-ren.jpg"
  },
  {
    id: "starter_mika_archive_layers",
    slug: "mika-archive-layers",
    name: "Mika - Archive Layers",
    image: "/landing/starters/starter-mika.jpg"
  }
] as const;

describe("starter character catalog", () => {
  it("includes the approved 8 fashion expansion presets in the fallback catalog", () => {
    expect(STARTER_CHARACTER_TEMPLATES).toHaveLength(23);

    for (const expected of expectedFashionPresets) {
      expect(STARTER_CHARACTER_TEMPLATES).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            id: expected.id,
            slug: expected.slug,
            name: expected.name,
            status: "setup_pending"
          })
        ])
      );
    }
  });

  it("keeps starter IDs, slugs, and sort orders unique", () => {
    const ids = STARTER_CHARACTER_TEMPLATES.map((starter) => starter.id);
    const slugs = STARTER_CHARACTER_TEMPLATES.map((starter) => starter.slug);
    const sorts = STARTER_CHARACTER_TEMPLATES.map((starter) => starter.sort);

    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(slugs).size).toBe(slugs.length);
    expect(new Set(sorts).size).toBe(sorts.length);
  });

  it("maps every new fashion preset to a public starter image", () => {
    const onboardingSource = readFileSync(resolve("src/client/screens/OnboardingScreen.tsx"), "utf8");

    for (const expected of expectedFashionPresets) {
      expect(onboardingSource).toContain(`${expected.id}: "${expected.image}"`);
      expect(existsSync(resolve(`public${expected.image}`))).toBe(true);
    }
  });
});
