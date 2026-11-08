// Server-side access to the gateway. The session token lives in an httpOnly cookie, so browser
// JavaScript never sees it; the browser talks to /api/v1/*, which forwards here.
import { cookies } from "next/headers";

export const GATEWAY = process.env.BRIER_GATEWAY_URL ?? "http://127.0.0.1:8080";
export const SESSION_COOKIE = "brier_session";
/** The gateway's WebSocket, which browsers connect to directly for public market data. */
export const WS_URL = process.env.BRIER_WS_URL ?? "ws://127.0.0.1:8080/v1/ws";

export async function sessionToken(): Promise<string | undefined> {
  return (await cookies()).get(SESSION_COOKIE)?.value;
}

export type Answer<T> = { ok: true; status: number; body: T } | { ok: false; status: number; body: ApiErrorBody };
export type ApiErrorBody = { error?: { code?: string; message?: string; seq?: number } };

/** Call the gateway from the server, with the caller's session if there is one. */
export async function gateway<T>(path: string, init: RequestInit = {}, token?: string): Promise<Answer<T>> {
  const headers = new Headers(init.headers);
  headers.set("content-type", "application/json");
  const t = token ?? (await sessionToken());
  if (t) headers.set("authorization", `Bearer ${t}`);
  let res: Response;
  try {
    res = await fetch(`${GATEWAY}${path}`, { ...init, headers, cache: "no-store" });
  } catch {
    // The exchange is unreachable: pages render their empty and error states instead of failing.
    return { ok: false, status: 503, body: { error: { code: "unavailable", message: "the exchange is not reachable" } } };
  }
  const text = await res.text();
  let body: unknown = null;
  try {
    body = text ? (JSON.parse(text) as unknown) : null;
  } catch {
    body = null;
  }
  return res.ok
    ? { ok: true, status: res.status, body: body as T }
    : { ok: false, status: res.status, body: (body ?? {}) as ApiErrorBody };
}

export type Me = { user_id: number; username: string; role: "trader" | "admin" | "resolver"; totp: boolean; signed_in_with_totp: boolean };

export async function me(): Promise<Me | null> {
  if (!(await sessionToken())) return null;
  const a = await gateway<Me>("/v1/me");
  return a.ok ? a.body : null;
}

/** The server's clock for one render (relative times like "closes in 3 days"). */
export function renderTime(): number {
  return Date.now();
}
