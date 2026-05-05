import type { Context } from "hono";
import type { AppBindings, AuthUser } from "../env";

export class HttpError extends Error {
  constructor(
    public readonly status: number,
    message: string,
    public readonly code = "request_error"
  ) {
    super(message);
  }
}

export function requireUser(c: Context<AppBindings>): AuthUser {
  const user = c.get("user");
  if (!user) {
    throw new HttpError(401, "Authentication required.", "auth_required");
  }
  return user;
}

export async function readJson(c: Context<AppBindings>): Promise<unknown> {
  try {
    return await c.req.json();
  } catch {
    throw new HttpError(400, "Expected a JSON request body.", "invalid_json");
  }
}

export function errorResponse(c: Context, error: unknown): Response {
  if (error instanceof HttpError) {
    return c.json({ error: { code: error.code, message: error.message } }, error.status as any);
  }

  const message = error instanceof Error ? error.message : "Unexpected server error.";
  console.error(error);
  return c.json({ error: { code: "internal_error", message } }, 500);
}
