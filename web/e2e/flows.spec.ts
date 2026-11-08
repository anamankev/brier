// The G6 exit flows, in order, through the browser, with an axe-core WCAG 2.2 AA check on every
// page. Elements are found by role and label, so the flows also check that the pages are
// operable without seeing them.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { PASSWORD, call, instant, iso, now, setClock, state, totp, until } from "./stack";

test.describe.configure({ mode: "serial" });

const NEWCOMER = `newcomer${Date.now() % 100000}`;
let newcomerSecret = "";

async function accessible(page: Page, what: string) {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
  const summary = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
  expect(summary, `${what} has accessibility violations`).toEqual([]);
}

async function signIn(page: Page, username: string, secret?: string) {
  await page.goto("/signin");
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Log in", exact: true }).click();
  if (secret) {
    await page.getByLabel("Authenticator code").fill(totp(secret, 1));
    await page.getByRole("button", { name: "Log in", exact: true }).click();
  }
  await expect(page.getByRole("link", { name: username })).toBeVisible();
}

const CPI = 101;

test("a newcomer signs up, turns on two-step sign-in, and signs in with a code", async ({ page }) => {
  await page.goto("/register");
  await accessible(page, "registration");
  await page.getByLabel(/Username/).fill(NEWCOMER);
  await page.getByLabel(/Password/).fill(PASSWORD);
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByRole("heading", { name: NEWCOMER })).toBeVisible();
  await accessible(page, "account");
  await page.getByRole("button", { name: "Turn on two-step sign-in" }).click();
  newcomerSecret = (await page.getByTestId("totp-secret").textContent()) ?? "";
  await page.getByLabel("6-digit code").fill(totp(newcomerSecret));
  await page.getByRole("button", { name: "Turn on", exact: true }).click();
  await expect(page.getByText("Two-step sign-in is on.")).toBeVisible();
  await page.getByRole("button", { name: "Sign out" }).click();
  await page.goto("/signin");
  await accessible(page, "sign-in");
  await signIn(page, NEWCOMER, newcomerSecret);
});

test("the market list shows the launch catalogue, open by schedule", async ({ page }) => {
  await until("the scheduler to open the markets", async () => {
    const { markets } = await call<{ markets: { state: string }[] }>("GET", "/v1/markets");
    return markets.length === 5 && markets.every((m) => m.state === "open");
  });
  await page.goto("/");
  await accessible(page, "market list");
  for (const m of state().markets) await expect(page.getByRole("link", { name: m.title })).toBeVisible();
});

test("a trade: buy YES from the ticket, fill against another trader, and cancel a resting order", async ({ page }) => {
  await signIn(page, NEWCOMER, newcomerSecret);
  await page.goto(`/markets/${CPI}`);
  await expect(page.getByText("● Live")).toBeVisible();
  await accessible(page, "market page");
  // Buy 10 YES at 55: it rests, and the live ladder shows the bid.
  await page.getByLabel(/Limit price/).fill("55");
  await page.getByLabel("Contracts").fill("10");
  await page.getByRole("button", { name: "Buy 10 Yes at 55¢" }).click();
  await expect(page.getByText(/0 of 10 filled; 10 resting/)).toBeVisible();
  await expect(page.getByRole("table", { name: "Bids to buy YES" }).getByRole("cell", { name: "55¢" })).toBeVisible();
  // Tom sells 10 YES at 55 (buys NO at 45): the order fills.
  await call("POST", "/v1/orders", state().users.tom!.token, { market: CPI, side: "sell", price: 55, qty: 10 });
  await page.goto("/portfolio");
  await accessible(page, "portfolio");
  await expect(page.getByRole("cell", { name: "10 Yes" })).toBeVisible();
  // A resting NO order, then cancelled from the portfolio.
  await page.goto(`/markets/${CPI}`);
  await page.getByRole("group", { name: "Outcome" }).getByRole("button", { name: /^No/ }).click();
  await page.getByLabel(/Limit price/).fill("20");
  await page.getByLabel("Contracts").fill("3");
  await page.getByRole("button", { name: "Buy 3 No at 20¢" }).click();
  await expect(page.getByText(/3 resting/)).toBeVisible();
  await page.goto("/portfolio");
  await expect(page.getByRole("cell", { name: "Buy No at 20¢" })).toBeVisible();
  await page.getByRole("button", { name: /Cancel order/ }).click();
  await expect(page.getByText("No open orders.")).toBeVisible();
});

test("a forecast is recorded, and covered by the next price sample", async ({ page }) => {
  await signIn(page, NEWCOMER, newcomerSecret);
  await page.goto(`/markets/${CPI}`);
  await page.getByLabel(/Your probability of YES/).fill("70");
  await page.getByRole("button", { name: /Save forecast|Update forecast/ }).click();
  await expect(page.getByText("Recorded: 70%.")).toBeVisible();
  // Two more minutes of trading, so samples cover the forecast.
  setClock(now() + 60);
  await new Promise((r) => setTimeout(r, 1500));
  setClock(now() + 60);
  await new Promise((r) => setTimeout(r, 1500));
});

test("resolution: an admin confirms in the console, a holder disputes, the dispute is rejected, and the market settles", async ({ page }) => {
  const s = state();
  const cpi = s.markets.find((m) => m.market === CPI)!;
  // Past the close and the release.
  setClock(instant(cpi.release) + 60);
  await until("market 101 to close", async () => {
    const { markets } = await call<{ markets: { market: number; state: string }[] }>("GET", "/v1/markets");
    return markets.find((m) => m.market === CPI)?.state === "closed";
  });
  // Ada proposes YES (directly: proposing is the resolver service's job, tested in Python).
  const evidence = { market: CPI, rules_hash: cpi.rules_hash, outcome: "yes", retrieved_at: iso(now()), source: "bls", value: "3.7" };
  await call("POST", `/v1/markets/${CPI}/resolution`, s.users.ada!.token, { outcome: "yes", evidence, reason: "BLS published November CPI" });

  // Cyd confirms it in the admin console.
  await signIn(page, "cyd", s.users.cyd!.secret);
  await page.goto("/admin");
  await accessible(page, "admin console");
  await page.getByLabel(/Reason/).fill("checked against the BLS release");
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText(/Confirm resolution \d+: done/)).toBeVisible();

  // The newcomer, who holds YES, disputes on the market page.
  await page.getByRole("button", { name: "Sign out" }).click();
  await signIn(page, NEWCOMER, newcomerSecret);
  await page.goto(`/markets/${CPI}`);
  await page.getByLabel(/Think this outcome is wrong/).fill("I think the release was revised");
  await page.getByRole("button", { name: "File a dispute" }).click();
  await expect(page.getByText(/Dispute filed/)).toBeVisible();

  // Cyd rejects the dispute.
  await page.getByRole("button", { name: "Sign out" }).click();
  await signIn(page, "cyd", s.users.cyd!.secret);
  await page.goto("/admin");
  await expect(page.getByRole("cell", { name: "I think the release was revised" })).toBeVisible();
  await page.getByLabel(/Reason/).fill("the rulebook counts the first release");
  await page.getByRole("button", { name: "Reject" }).click();
  await expect(page.getByText(/Reject dispute \d+: done/)).toBeVisible();

  // The window ends; the gateway settles.
  setClock(now() + 3_601);
  await until("settlement", async () => {
    const view = await call<{ resolutions: { state: string }[]; journal: unknown }>("GET", `/v1/markets/${CPI}/resolution`);
    return view.resolutions[0]?.state === "settled" && view.journal !== null;
  });
  await page.goto(`/markets/${CPI}`);
  await expect(page.getByText("settled", { exact: true })).toBeVisible();
  await page.getByText("Evidence record").first().click();
  await expect(page.getByText("Matches the exchange log")).toBeVisible();
});

test("the scorecard and leaderboards show the result", async ({ page }) => {
  await signIn(page, NEWCOMER, newcomerSecret);
  const title = state().markets.find((m) => m.market === CPI)!.title;
  // The scorer runs on the gateway's next pass after the settlement reaches the read models.
  await expect(async () => {
    await page.goto("/scorecard");
    await expect(page.getByRole("link", { name: title })).toBeVisible({ timeout: 1_000 });
  }).toPass({ timeout: 20_000 });
  await accessible(page, "scorecard");
  await page.goto("/leaderboards");
  await accessible(page, "leaderboards");
  // Bought 10 YES at 55, YES won: +450 credits.
  await expect(page.getByRole("row", { name: new RegExp(`${NEWCOMER}.*\\+4\\.50`) })).toBeVisible();
});
