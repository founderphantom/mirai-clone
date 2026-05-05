export type JsonRecord = Record<string, unknown>;

export const nowIso = () => new Date().toISOString();

export const createId = (prefix: string) => `${prefix}_${crypto.randomUUID().replaceAll("-", "")}`;

export async function first<T>(
  db: D1Database,
  sql: string,
  params: unknown[] = []
): Promise<T | null> {
  return await db.prepare(sql).bind(...params.map(normalizeParam)).first<T>();
}

export async function all<T>(
  db: D1Database,
  sql: string,
  params: unknown[] = []
): Promise<T[]> {
  const result = await db.prepare(sql).bind(...params.map(normalizeParam)).all<T>();
  return result.results ?? [];
}

export async function run(
  db: D1Database,
  sql: string,
  params: unknown[] = []
): Promise<D1Result> {
  return await db.prepare(sql).bind(...params.map(normalizeParam)).run();
}

export function parseJson<T>(value: string | null | undefined, fallback: T): T {
  if (!value) return fallback;
  try {
    return JSON.parse(value) as T;
  } catch {
    return fallback;
  }
}

export function toJson(value: unknown): string {
  return JSON.stringify(value ?? {});
}

export function slugify(value: string): string {
  return value
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
}

export async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const hash = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function safeR2Segment(value: string): string {
  return value.replace(/[^a-zA-Z0-9._-]/g, "-").slice(0, 96);
}

function normalizeParam(value: unknown): string | number | null {
  if (value === undefined) return null;
  if (value === null) return null;
  if (typeof value === "boolean") return value ? 1 : 0;
  if (typeof value === "number" || typeof value === "string") return value;
  return JSON.stringify(value);
}
