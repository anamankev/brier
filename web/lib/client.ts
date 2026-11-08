// Browser-side calls to /api/v1/* (the server forwards them with the session cookie).
export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: { "content-type": "application/json", "x-brier-request": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: "no-store",
  });
  const text = await res.text();
  const json = text ? (JSON.parse(text) as unknown) : null;
  if (!res.ok) {
    const e = (json as { error?: { code?: string; message?: string } } | null)?.error;
    throw new ApiError(res.status, e?.code ?? "error", e?.message ?? `HTTP ${res.status}`);
  }
  return json as T;
}

/** A random client order ID (53 bits, safe in JSON), so a retried order is not placed twice. */
export function clientOrderId(): number {
  const words = crypto.getRandomValues(new Uint32Array(2));
  return (words[0]! & 0x1fffff) * 2 ** 32 + words[1]!;
}
