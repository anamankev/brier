// The browser's gateway: /api/v1/* forwards to the gateway's /v1/* with the session from the
// httpOnly cookie. A request that changes anything must carry `x-brier-request: 1`, which a
// cross-site form or image cannot send (with the SameSite=Strict cookie, a CSRF defence).
import { NextResponse, type NextRequest } from "next/server";
import { GATEWAY, proxyHeaders, sessionToken } from "@/lib/server";

async function forward(req: NextRequest, ctx: { params: Promise<{ path: string[] }> }) {
  const { path } = await ctx.params;
  if (req.method !== "GET" && req.headers.get("x-brier-request") !== "1") {
    return NextResponse.json({ error: { code: "forbidden", message: "missing request header" } }, { status: 403 });
  }
  const target = `${GATEWAY}/v1/${path.map(encodeURIComponent).join("/")}${req.nextUrl.search}`;
  const headers: Record<string, string> = { "content-type": "application/json", ...(await proxyHeaders()) };
  const token = await sessionToken();
  if (token) headers.authorization = `Bearer ${token}`;
  const body = req.method === "GET" || req.method === "DELETE" ? undefined : await req.text();
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
