import { all, createId, first, nowIso, parseJson, run, slugify, toJson } from "../db";
import type { Env } from "../env";

export type BubbleCandidate = {
  slug: string;
  title: string;
  vibeSummary: string;
  searchQueries: string[];
  exampleKeywords: string[];
};

type CloneForPersona = {
  id: string;
  user_id: string;
  name: string;
  persona: string;
  style_prompt: string;
  soul_source: string;
  source_snapshot_json: string;
};

export async function ensurePersonaBubblesForClone(env: Env, userId: string, cloneId: string) {
  const existing = await all<any>(
    env.DB,
    `SELECT * FROM inspiration_bubbles WHERE user_id = ? AND clone_id = ? ORDER BY sort ASC`,
    [userId, cloneId]
  );
  if (existing.length > 0) return existing;

  const clone = await first<CloneForPersona>(
    env.DB,
    `SELECT id, user_id, name, persona, style_prompt, soul_source, source_snapshot_json
     FROM clone_profiles
     WHERE id = ? AND user_id = ?`,
    [cloneId, userId]
  );
  if (!clone) return [];

  const candidates = await synthesizeBubbles(env, clone);
  const createdAt = nowIso();
  for (const [index, bubble] of candidates.slice(0, 20).entries()) {
    await run(
      env.DB,
      `INSERT INTO inspiration_bubbles
        (id, user_id, clone_id, slug, title, vibe_summary, search_queries_json,
         example_keywords, source, selected, weight, sort, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        createId("bubble"),
        userId,
        cloneId,
        bubble.slug,
        bubble.title,
        bubble.vibeSummary,
        toJson(bubble.searchQueries),
        bubble.exampleKeywords.join(", "),
        "persona_agent",
        0,
        1,
        index + 1,
        createdAt
      ]
    );
  }

  return await all<any>(
    env.DB,
    `SELECT * FROM inspiration_bubbles WHERE user_id = ? AND clone_id = ? ORDER BY sort ASC`,
    [userId, cloneId]
  );
}

async function synthesizeBubbles(env: Env, clone: CloneForPersona): Promise<BubbleCandidate[]> {
  if (env.ANTHROPIC_API_KEY) {
    const ai = await synthesizeWithAnthropic(env, clone);
    if (ai.length >= 8) return ai;
  }
  return fallbackBubbles(clone);
}

async function synthesizeWithAnthropic(env: Env, clone: CloneForPersona): Promise<BubbleCandidate[]> {
  const apiKey = env.ANTHROPIC_API_KEY;
  if (!apiKey) return [];

  const prompt = {
    clone: {
      name: clone.name,
      persona: clone.persona,
      stylePrompt: clone.style_prompt,
      source: clone.soul_source,
      sourceSnapshot: parseJson(clone.source_snapshot_json, {})
    },
    instruction:
      "Return JSON only: 10 to 20 objects with slug, title, vibeSummary, searchQueries, exampleKeywords for IG/TikTok creator inspiration bubbles."
  };

  try {
    const response = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers: {
        "x-api-key": apiKey,
        "anthropic-version": "2023-06-01",
        "content-type": "application/json"
      },
      body: JSON.stringify({
        model: env.ANTHROPIC_PERSONA_MODEL || "claude-haiku-4-5-20251001",
        max_tokens: 1600,
        messages: [{ role: "user", content: JSON.stringify(prompt) }]
      })
    });
    if (!response.ok) return [];

    const body = (await response.json()) as any;
    const text = body?.content?.find((item: any) => item?.type === "text")?.text;
    const parsed = typeof text === "string" ? JSON.parse(text) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed.map(normalizeBubble).filter(Boolean) as BubbleCandidate[];
  } catch {
    return [];
  }
}

function fallbackBubbles(clone: CloneForPersona): BubbleCandidate[] {
  const source = clone.soul_source || "creator";
  const personaText = `${clone.persona} ${clone.style_prompt}`.toLowerCase();
  const base = [
    ["Y2K Cafe", "Playful cafe looks with chrome accessories and flash-photo confidence"],
    ["Tokyo Neon", "Night city color, glossy styling, and bright social hooks"],
    ["Cottagecore Picnic", "Soft outdoor scenes, florals, blankets, and warm daylight"],
    ["Clean Girl Errands", "Minimal athleisure, errands, matcha stops, and clear morning light"],
    ["Rooftop Golden Hour", "Elevated city views, warm light, and polished lifestyle poses"],
    ["Pilates Morning", "Wellness studio energy, activewear sets, and calm routine content"],
    ["Streetwear Fit Check", "Layered outfits, sneaker details, and confident city framing"],
    ["Coastal Weekend", "Beach walks, linen textures, sunglasses, and relaxed luxury"],
    ["Editorial Flash", "Magazine-style makeup, strong silhouettes, and direct-camera energy"],
    ["Festival Night", "Statement outfits, motion blur, flash, and crowd atmosphere"],
    ["Gallery Date", "Quiet luxury outfits, white walls, clean composition, and art spaces"],
    ["Airport Lounge", "Travel-day outfit, luggage, lounges, and aspirational movement"]
  ];

  const boosted = personaText.includes("fitness")
    ? ["Pilates Morning", "Clean Girl Errands"]
    : personaText.includes("coastal")
      ? ["Coastal Weekend", "Rooftop Golden Hour"]
      : personaText.includes("street")
        ? ["Streetwear Fit Check", "Tokyo Neon"]
        : [];

  return base
    .sort((a, b) => boosted.indexOf(b[0]) - boosted.indexOf(a[0]))
    .map(([title, summary]) =>
      normalizeBubble({
        title,
        slug: slugify(title),
        vibeSummary: summary,
        searchQueries: [`${title} ${source} creator`, `${title} instagram reels`, `${title} tiktok lifestyle`],
        exampleKeywords: title.toLowerCase().split(/\s+/)
      })
    )
    .filter(Boolean) as BubbleCandidate[];
}

function normalizeBubble(value: any): BubbleCandidate | null {
  if (!value || typeof value !== "object") return null;
  const title = String(value.title || "").slice(0, 80).trim();
  if (!title) return null;
  const slug = slugify(String(value.slug || title)) || slugify(title);
  const searchQueries = arrayOfStrings(value.searchQueries || value.search_queries).slice(0, 5);
  return {
    slug,
    title,
    vibeSummary: String(value.vibeSummary || value.vibe_summary || "").slice(0, 400),
    searchQueries: searchQueries.length > 0 ? searchQueries : [`${title} creator inspiration`],
    exampleKeywords: arrayOfStrings(value.exampleKeywords || value.example_keywords).slice(0, 8)
  };
}

function arrayOfStrings(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    : [];
}
