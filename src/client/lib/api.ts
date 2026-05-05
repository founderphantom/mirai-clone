export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: "include",
    ...options,
    headers: {
      ...(options.body instanceof FormData ? {} : { "content-type": "application/json" }),
      ...options.headers
    }
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null) as unknown;
    const message = readApiErrorMessage(body);
    throw new Error(message || `Request failed with ${response.status}`);
  }

  return (await response.json()) as T;
}

function readApiErrorMessage(body: unknown): string | null {
  if (!body || typeof body !== "object") return null;
  const error = (body as { error?: unknown }).error;
  if (!error || typeof error !== "object") return null;
  const message = (error as { message?: unknown }).message;
  return typeof message === "string" ? message : null;
}
