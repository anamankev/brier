// The web server's defences (security review, 12 November 2026): a per-request script nonce,
// cross-site and unmarked writes refused, no climbing out of /v1, a locked-down session
// cookie, the production settings check, and the reporting policy.
import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { envProblems } from "../instrumentation";
import { NEWCOMER_SESSION } from "./stack";

test("every page gets its own script nonce, and inline scripts without it are refused", async ({ page, request }) => {
  const a = (await request.get("/")).headers()["content-security-policy"] ?? "";
  const b = (await request.get("/")).headers()["content-security-policy"] ?? "";
  const nonce = (p: string) => /'nonce-([^']+)'/.exec(p)?.[1];
  expect(nonce(a), a).toBeTruthy();
  expect(nonce(a)).not.toBe(nonce(b));
  const scripts = /script-src ([^;]+)/.exec(a)?.[1] ?? "";
  expect(scripts).not.toContain("unsafe-inline");
  expect(a).toContain("object-src 'none'");
  expect(a).toContain("frame-ancestors 'none'");
  // The page still works under it, and a script injected without the nonce does not run.
  const violations: string[] = [];
  page.on("console", (m) => {
    if (m.type() === "error" && /Content Security Policy/i.test(m.text())) violations.push(m.text());
  });
  await page.goto("/");
  await expect(page.getByRole("link", { name: "Brier home" }).first()).toBeVisible();
  // What an HTML injection would add: an inline event handler, which needs no script tag.
  await page.evaluate(() => {
    const div = document.createElement("div");
    div.innerHTML = '<img src="/no-such-image" onerror="window.__injected = true">';
    document.body.appendChild(div);
  });
  await page.waitForTimeout(500);
  const ran = await page.evaluate(() => (window as unknown as { __injected?: boolean }).__injected === true);
  expect(ran, "an injected inline handler must not run").toBe(false);
  expect(violations.some((v) => v.includes("script")), "the blocked script is reported").toBe(true);
});

test("the other security headers are set", async ({ request }) => {
  const h = (await request.get("/")).headers();
  expect(h["x-content-type-options"]).toBe("nosniff");
  expect(h["x-frame-options"]).toBe("DENY");
  expect(h["referrer-policy"]).toBe("same-origin");
  expect(h["permissions-policy"]).toContain("camera=()");
  expect(h["cross-origin-opener-policy"]).toBe("same-origin");
  expect(h["x-powered-by"]).toBeUndefined();
});

test("writes from another site, or without the request header, are refused", async ({ request }) => {
  const body = { username: "nobody_here", password: "not the password at all" };
  const unmarked = await request.post("/api/session", { data: body });
  expect(unmarked.status()).toBe(403);
  const crossSite = await request.post("/api/session", {
    data: body,
    headers: { "x-brier-request": "1", origin: "https://evil.example" },
  });
  expect(crossSite.status()).toBe(403);
  const proxied = await request.post("/api/v1/allowance", { headers: { "x-brier-request": "1", origin: "https://evil.example" } });
  expect(proxied.status()).toBe(403);
  const sameSite = await request.post("/api/session", { data: body, headers: { "x-brier-request": "1" } });
  expect(sameSite.status(), "a same-site sign-in reaches the gateway").toBe(401);
});

test("the proxy cannot be steered out of /v1", async ({ request }) => {
  // URL parsing folds encoded dots before the route sees them, so these land outside the proxy
  // (a 404 from the web app) rather than on the gateway outside /v1.
  for (const path of ["/api/v1/../../health", "/api/v1/%2e%2e/%2e%2e/v1/health", "/api/v1/events/%2e%2e/%2e%2e/%2e%2e/admin/audit"]) {
    const res = await request.get(path);
    expect([400, 404], path).toContain(res.status());
  }
  // A dot segment that survives as a parameter is refused by the route itself.
  const encoded = await request.get("/api/v1/events/%252e%252e");
  expect(encoded.status()).not.toBe(500);
});

test("the session cookie is host-only, Secure, httpOnly and SameSite=Strict", () => {
  const state = JSON.parse(readFileSync(NEWCOMER_SESSION, "utf8")) as { cookies: { name: string; httpOnly: boolean; secure: boolean; sameSite: string; domain: string }[] };
  const cookie = state.cookies.find((c) => c.name.endsWith("brier_session"));
  expect(cookie, JSON.stringify(state.cookies.map((c) => c.name))).toBeTruthy();
  expect(cookie!.name).toBe("__Host-brier_session");
  expect([cookie!.httpOnly, cookie!.secure, cookie!.sameSite]).toEqual([true, true, "Strict"]);
  expect(cookie!.domain.startsWith("."), "host-only").toBe(false);
});

test("production refuses to start with missing or weak settings", () => {
  expect(envProblems({})).toHaveLength(4);
  expect(
    envProblems({
      BRIER_GATEWAY_URL: "http://10.0.0.138:8080",
      BRIER_WS_URL: "ws://example.org/v1/ws",
      BRIER_SITE_URL: "http://example.org",
      BRIER_PROXY_KEY: "short",
    }),
  ).toEqual(["BRIER_WS_URL must use wss:", "BRIER_SITE_URL must use https:", "BRIER_PROXY_KEY must be at least 32 characters"]);
  expect(
    envProblems({
      BRIER_GATEWAY_URL: "http://10.0.0.138:8080",
      BRIER_WS_URL: "wss://example.org/v1/ws",
      BRIER_SITE_URL: "https://example.org",
      BRIER_PROXY_KEY: "k".repeat(64),
    }),
  ).toEqual([]);
});

test("security.txt says where to report a problem", async ({ request }) => {
  const res = await request.get("/.well-known/security.txt");
  expect(res.status()).toBe(200);
  expect(await res.text()).toContain("Contact: https://github.com/anamankev/brier/security/advisories/new");
});
