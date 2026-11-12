// Sign in and out. The gateway's session token is kept in an httpOnly, SameSite=Strict cookie.
import { NextResponse, type NextRequest } from "next/server";
import { cookies } from "next/headers";
import { GATEWAY, SESSION_COOKIE, proxyHeaders, sameSiteWrite } from "@/lib/server";

const MAX_AGE = 24 * 3600; // the gateway's session lifetime

/** The cookie's attributes. A __Host- cookie can only be replaced or removed with the same
 * Secure and path settings, so setting and deleting share them. */
const ATTRIBUTES = {
  httpOnly: true,
  sameSite: "strict",
  secure: process.env.NODE_ENV === "production",
  path: "/",
  priority: "high",
} as const;

export async function POST(req: NextRequest) {
  if (!sameSiteWrite(req)) {
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
  (await cookies()).set(SESSION_COOKIE, body.token, { ...ATTRIBUTES, maxAge: MAX_AGE });
  return NextResponse.json({ user_id: body.user_id });
}

export async function DELETE(req: NextRequest) {
  if (!sameSiteWrite(req)) {
    return NextResponse.json({ error: { code: "forbidden", message: "missing request header" } }, { status: 403 });
  }
  const jar = await cookies();
  const token = jar.get(SESSION_COOKIE)?.value;
  if (token) {
    await fetch(`${GATEWAY}/v1/logout`, { method: "POST", headers: { authorization: `Bearer ${token}` }, cache: "no-store" });
  }
  jar.set(SESSION_COOKIE, "", { ...ATTRIBUTES, maxAge: 0 });
  return new NextResponse(null, { status: 204 });
}
