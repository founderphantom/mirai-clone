import { all, createId, parseJson, run } from "../db";
import type { Env } from "../env";
import { getDiscoveryFeed } from "../discovery/scrapecreators";

type BubbleRow = {
  id: string;
  user_id: string;
  clone_id: string | null;
  search_queries_json: string;
  selected: number;
};

export async function seedInspirationPool(
  env: Env,
  userId: string,
  cloneId: string,
  bubbleIds?: string[]
) {
  const bubbles = await selectedBubbles(env, userId, cloneId, bubbleIds);
  let linked = 0;

  for (const bubble of bubbles) {
    const queries = parseJson<string[]>(bubble.search_queries_json, []).slice(0, 2);
    for (const query of queries) {
      const items = await discoverySearch(env, query);
      for (const item of items) {
        const result = await run(
          env.DB,
          `INSERT OR IGNORE INTO user_inspiration_pool
            (id, user_id, bubble_id, discovery_item_id, score, used_at, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          [createId("pool"), userId, bubble.id, item.id, 1, null, new Date().toISOString()]
        );
        linked += result.meta.changes ?? 0;
      }
    }
  }

  return { bubbleCount: bubbles.length, linked };
}

async function selectedBubbles(
  env: Env,
  userId: string,
  cloneId: string,
  bubbleIds?: string[]
): Promise<BubbleRow[]> {
  if (bubbleIds && bubbleIds.length > 0) {
    const placeholders = bubbleIds.map(() => "?").join(", ");
    return await all<BubbleRow>(
      env.DB,
      `SELECT id, user_id, clone_id, search_queries_json, selected
       FROM inspiration_bubbles
       WHERE user_id = ? AND clone_id = ? AND id IN (${placeholders})
       ORDER BY sort ASC`,
      [userId, cloneId, ...bubbleIds]
    );
  }

  return await all<BubbleRow>(
    env.DB,
    `SELECT id, user_id, clone_id, search_queries_json, selected
     FROM inspiration_bubbles
     WHERE user_id = ? AND clone_id = ? AND selected = 1
     ORDER BY sort ASC`,
    [userId, cloneId]
  );
}

async function discoverySearch(env: Env, query: string): Promise<Array<{ id: string }>> {
  try {
    const response = await getDiscoveryFeed(env, {
      source: "instagram-reels",
      query,
      limit: 20,
      force: false
    });
    return Array.isArray(response.items) ? response.items.map((item: any) => ({ id: item.id })) : [];
  } catch {
    return [];
  }
}
