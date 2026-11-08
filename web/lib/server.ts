// Server-side access to the gateway. The session token lives in an httpOnly cookie, so browser
// JavaScript never sees it; the browser talks to /api/v1/*, which forwards here.
import { cookies } from "next/headers";

export const GATEWAY = process.env.BRIER_GATEWAY_URL ?? "http://127.0.0.1:8080";
export const SESSION_COOKIE = "brier_session";

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
  const res = await fetch(`${GATEWAY}${path}`, { ...init, headers, cache: "no-store" });
  const text = await res.text();
  const body = text ? (JSON.parse(text) as unknown) : null;
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
