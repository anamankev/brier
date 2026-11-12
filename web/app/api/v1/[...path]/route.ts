// The browser's gateway: /api/v1/* forwards to the gateway's /v1/* with the session from the
// httpOnly cookie. A request that changes anything must carry `x-brier-request: 1`, which a
// cross-site form or image cannot send, and come from this site's origin (with the
// SameSite=Strict cookie, the CSRF defences).
import { NextResponse, type NextRequest } from "next/server";
import { GATEWAY, proxyHeaders, sameSiteWrite, sessionToken } from "@/lib/server";

async function forward(req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) {
  const { path } = await ctx.params;
  if (req.method !== "GET" && !sameSiteWrite(req)) {
    return NextResponse.json({ error: { code: "forbidden", message: "cross-site or unmarked request" } }, { status: 403 });
  }
  // Only plain path segments reach the gateway: no "." or ".." to climb out of /v1.
  if (path.some((p) => p === "." || p === ".." || p === "")) {
    return NextResponse.json({ error: { code: "not_found", message: "no such endpoint" } }, { status: 404 });
  }
  const target = `${GATEWAY}/v1/${path.map(encodeURIComponent).join("/")}${req.nextUrl.search}`;
  const headers: Record<string, string> = { "content-type": "application/json", ...(await proxyHeaders()) };
  const token = await sessionToken();
  if (token) headers.authorization = `Bearer ${token}`;
  const body = req.method === "GET" ? undefined : (await req.text()) || undefined;
  const res = await fetch(target, { method: req.method, headers, body, cache: "no-store" });
  const text = await res.text();
  return new NextResponse(text || null, {
    status: res.status,
    headers: { "content-type": res.headers.get("content-type") ?? "application/json" },
  });
}

export const GET = forward;
export const POST = forward;
export const PUT = forward;
export const PATCH = forward;
export const DELETE = forward;
