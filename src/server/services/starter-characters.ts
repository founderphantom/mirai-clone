import { all, first, parseJson } from "../db";
import type { AuthUser, Env } from "../env";
import { HttpError } from "../http/errors";
import { createOnboardingClone } from "./onboarding-clones";

export type StarterCharacter = {
  id: string;
  slug: string;
  name: string;
  persona: string;
  style_prompt: string;
  hero_media_id: string | null;
  provider_config_json: string;
  sort: number;
  status: string;
};

export const STARTER_CHARACTER_TEMPLATES: StarterCharacter[] = [
  starter("starter_amara_cherry_grwm", "amara-cherry-grwm", "Amara - Cherry GRWM", "Afro-Latina beauty creator with glossy makeup, apartment GRWM clips, lace details, and confident city mornings", 1),
  starter("starter_priya_resort_edit", "priya-resort-edit", "Priya - Resort Edit", "South Asian luxury travel creator with boutique hotels, linen outfits, golden-hour balconies, and elevated vacation styling", 2),
  starter("starter_miles_streetwear_lens", "miles-streetwear-lens", "Miles - Streetwear Lens", "streetwear and sneaker creator with downtown daylight, layered blue and khaki fits, candid fit checks, and cool city energy", 3),
  starter("starter_hana_seoul_skin", "hana-seoul-skin", "Hana - Seoul Skin", "K-beauty and soft fashion creator with cafe routines, dewy skincare, cozy knits, and clean editorial details", 4),
  starter("starter_leila_fragrance_editorial", "leila-fragrance-editorial", "Leila - Fragrance Editorial", "Middle Eastern fragrance and fashion creator with rooftop dusk portraits, poetcore lace, brooch accents, and cinematic travel mood", 5),
  starter("starter_sky_soft_glam", "sky-soft-glam", "Sky - Soft Glam", "soft glam lifestyle creator", 10),
  starter("starter_marina_coastal", "marina-coastal", "Marina - Coastal", "coastal lifestyle creator", 20),
  starter("starter_aiden_streetwear", "aiden-streetwear", "Aiden - Streetwear", "streetwear fashion creator", 30),
  starter("starter_noor_editorial", "noor-editorial", "Noor - Editorial", "bold editorial creator", 40),
  starter("starter_juno_fitness", "juno-fitness", "Juno - Fitness", "wellness and fitness creator", 50),
  starter("starter_valentin_luxury_travel", "valentin-luxury-travel", "Valentin - Luxury Travel", "luxury travel creator", 60),
  starter("starter_sienna_cottagecore", "sienna-cottagecore", "Sienna - Cottagecore", "cottagecore lifestyle creator", 70),
  starter("starter_kai_cyber_night", "kai-cyber-night", "Kai - Cyber Night", "neon nightlife creator", 80),
  starter("starter_maya_minimal_clean", "maya-minimal-clean", "Maya - Minimal Clean", "minimal clean lifestyle creator", 90),
  starter("starter_rio_festival", "rio-festival", "Rio - Festival", "festival and nightlife creator", 100),
  starter("starter_rowan_parsons_crit", "rowan-parsons-crit", "Rowan - Parsons Crit", "experimental art-school fashion creator with studio critiques, handmade layers, gallery stairwells, and smart downtown styling", 110),
  starter("starter_liora_downtown_muse", "liora-downtown-muse", "Liora - Downtown Muse", "downtown fashion creator with vintage slips, sharp coats, gallery openings, mirror fit checks, and late-afternoon city light", 120),
  starter("starter_dorian_experimental_dresser", "dorian-experimental-dresser", "Dorian - Experimental Dresser", "experimental menswear creator with sculptural outerwear, wide trousers, unusual proportions, and quiet confidence in city architecture", 130),
  starter("starter_sable_form_study", "sable-form-study", "Sable - Form Study", "form-led fashion creator with monochrome layering, textile closeups, cropped poses, and outfit-first editorial compositions", 140),
  starter("starter_vesper_goth_rap_streetwear", "vesper-goth-rap-streetwear", "Vesper - Goth Rap Streetwear", "goth-rap avant streetwear creator with black leather, oversized silhouettes, silver hardware, night flash, and distorted club-adjacent energy", 150),
  starter("starter_niko_underground_set", "niko-underground-set", "Niko - Underground Set", "underground music fashion creator with basement show fits, worn denim, layered merch, film-grain flash, and post-show street portraits", 160),
  starter("starter_ren_niche_designer", "ren-niche-designer", "Ren - Niche Designer", "niche designer fashion creator with asymmetrical cuts, technical fabrics, quiet Tokyo side streets, and collector-level wardrobe details", 170),
  starter("starter_mika_archive_layers", "mika-archive-layers", "Mika - Archive Layers", "archive fashion creator with rare jackets, layered skirts and trousers, museum-library textures, and thoughtful outfit breakdowns", 180)
];

export async function listStarterCharacters(env: Env): Promise<StarterCharacter[]> {
  try {
    const rows = await all<StarterCharacter>(
      env.DB,
      `SELECT id, slug, name, persona, style_prompt, hero_media_id, provider_config_json, sort, status
       FROM starter_characters
       ORDER BY sort ASC`
    );
    if (rows.length === 0) return STARTER_CHARACTER_TEMPLATES;
    const byId = new Map(STARTER_CHARACTER_TEMPLATES.map((starter) => [starter.id, starter]));
    rows.forEach((row) => byId.set(row.id, row));
    return [...byId.values()].sort((left, right) => left.sort - right.sort);
  } catch {
    return STARTER_CHARACTER_TEMPLATES;
  }
}

export async function adoptStarterCharacter(env: Env, user: AuthUser, starterId: string) {
  const starter = await getStarterCharacter(env, starterId);
  const clone = await createOnboardingClone(env, user, {
    name: starter.name,
    handleBase: starter.slug,
    persona: starter.persona,
    stylePrompt: starter.style_prompt,
    source: "starter",
    starterCharacterId: starter.id,
    providerConfig: parseJson(starter.provider_config_json, {}),
    sourceSnapshot: {
      starterCharacterId: starter.id,
      starterSlug: starter.slug,
      starterStatus: starter.status,
      note: "Starter Soul provider assets will be wired when the preset Souls are set up."
    },
    soulStatus: hasProviderCharacter(starter.provider_config_json) ? "ready" : "pending_script"
  });

  return { clone, starter };
}

async function getStarterCharacter(env: Env, starterId: string): Promise<StarterCharacter> {
  const byDb = await first<StarterCharacter>(
    env.DB,
    `SELECT id, slug, name, persona, style_prompt, hero_media_id, provider_config_json, sort, status
     FROM starter_characters
     WHERE id = ? OR slug = ?`,
    [starterId, starterId]
  );
  const starter = byDb ?? STARTER_CHARACTER_TEMPLATES.find((item) => item.id === starterId || item.slug === starterId);
  if (!starter) throw new HttpError(404, "Starter Soul was not found.", "starter_not_found");
  return starter;
}

function hasProviderCharacter(providerConfigJson: string): boolean {
  const config = parseJson<Record<string, unknown>>(providerConfigJson, {});
  return typeof config.characterId === "string" || typeof config.soulCharacterId === "string";
}

function starter(id: string, slug: string, name: string, persona: string, sort: number): StarterCharacter {
  return {
    id,
    slug,
    name,
    persona,
    style_prompt: `${persona}, social-first composition, trend-ready lifestyle image`,
    hero_media_id: null,
    provider_config_json: "{}",
    sort,
    status: "setup_pending"
  };
}
