// Server-side access to the gateway. The session token lives in an httpOnly cookie, so browser
// JavaScript never sees it; the browser talks to /api/v1/*, which forwards here.
import { cookies, headers as requestHeaders } from "next/headers";

export const GATEWAY = process.env.BRIER_GATEWAY_URL ?? "http://127.0.0.1:8080";
/** In production the __Host- prefix makes the browser refuse the cookie unless it is Secure,
 * host-only and for the whole site, so a sibling domain or plain HTTP can never set it. */
export const SESSION_COOKIE = process.env.NODE_ENV === "production" ? "__Host-brier_session" : "brier_session";
/** The gateway's WebSocket, which browsers connect to directly for public market data. */
export const WS_URL = process.env.BRIER_WS_URL ?? "ws://127.0.0.1:8080/v1/ws";

export async function sessionToken(): Promise<string | undefined> {
  return (await cookies()).get(SESSION_COOKIE)?.value;
}

/**
 * Headers that let the gateway trust the client address this server saw. Set BRIER_PROXY_KEY
 * (the gateway's key) and BRIER_CLIENT_ADDRESS_HEADER (the header an edge proxy in front of this
 * server overwrites with the client's address, such as cf-connecting-ip). Never x-forwarded-for
 * as sent by a browser: anyone can set it.
 */
export async function proxyHeaders(): Promise<Record<string, string>> {
  const key = process.env.BRIER_PROXY_KEY;
  if (!key) return {};
  const out: Record<string, string> = { "x-brier-proxy-key": key };
  const name = process.env.BRIER_CLIENT_ADDRESS_HEADER;
  const address = name ? (await requestHeaders()).get(name)?.split(",")[0]?.trim() : undefined;
  if (address) out["x-brier-client-address"] = address;
  return out;
}

export type Answer<T> = { ok: true; status: number; body: T } | { ok: false; status: number; body: ApiErrorBody };
export type ApiErrorBody = { error?: { code?: string; message?: string; seq?: number } };

/** Call the gateway from the server, with the caller's session if there is one. */
export async function gateway<T>(path: string, init: RequestInit = {}, token?: string): Promise<Answer<T>> {
  const headers = new Headers(init.headers);
  headers.set("content-type", "application/json");
  for (const [k, v] of Object.entries(await proxyHeaders())) headers.set(k, v);
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

export type Me = { user_id: number; username: string; role: "trader" | "admin" | "resolver" | "surveillance"; totp: boolean; signed_in_with_totp: boolean };

export async function me(): Promise<Me | null> {
  if (!(await sessionToken())) return null;
  const a = await gateway<Me>("/v1/me");
  return a.ok ? a.body : null;
}

/** The server's clock for one render (relative times like "closes in 3 days"). */
export function renderTime(): number {
  return Date.now();
}

/**
 * Whether a request that changes something may proceed: it must carry `x-brier-request: 1`
 * (a cross-site form or image cannot), and if the browser names its origin, that origin must be
 * this site. With the SameSite=Strict cookie these are the CSRF defences.
 */
export function sameSiteWrite(req: Request): boolean {
  if (req.headers.get("x-brier-request") !== "1") return false;
  const origin = req.headers.get("origin");
  if (origin === null) return true;
  try {
    return new URL(origin).host === req.headers.get("host");
  } catch {
    return false;
  }
}
