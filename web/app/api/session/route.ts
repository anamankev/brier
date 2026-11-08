// Sign in and out. The gateway's session token is kept in an httpOnly, SameSite=Strict cookie.
import { NextResponse, type NextRequest } from "next/server";
import { cookies } from "next/headers";
import { GATEWAY, SESSION_COOKIE, proxyHeaders } from "@/lib/server";

const MAX_AGE = 24 * 3600; // the gateway's session lifetime

export async function POST(req: NextRequest) {
  if (req.headers.get("x-brier-request") !== "1") {
    return NextResponse.json({ error: { code: "forbidden", message: "missing request header" } }, { status: 403 });
  }
  const res = await fetch(`${GATEWAY}/v1/login`, {
    method: "POST",
    headers: { "content-type": "application/json", ...(await proxyHeaders()) },
    body: await req.text(),
    cache: "no-store",
  });
  const body = (await res.json()) as { token?: string; user_id?: number };
  if (!res.ok || !body.token) return NextResponse.json(body, { status: res.status });
  (await cookies()).set(SESSION_COOKIE, body.token, {
    httpOnly: true,
    sameSite: "strict",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: MAX_AGE,
  });
  return NextResponse.json({ user_id: body.user_id });
}

export async function DELETE(req: NextRequest) {
  if (req.headers.get("x-brier-request") !== "1") {
    return NextResponse.json({ error: { code: "forbidden", message: "missing request header" } }, { status: 403 });
  }
  const jar = await cookies();
  const token = jar.get(SESSION_COOKIE)?.value;
  if (token) {
    await fetch(`${GATEWAY}/v1/logout`, { method: "POST", headers: { authorization: `Bearer ${token}` }, cache: "no-store" });
  }
  jar.delete(SESSION_COOKIE);
  return new NextResponse(null, { status: 204 });
}
