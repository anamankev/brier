// The G6 exit flows, in order, through the browser, with an axe-core WCAG 2.2 AA check on every
// page. Elements are found by role and label, so the flows also check that the pages can be used
// without seeing them. The newcomer signs in once and keeps the session, as a browser would; TOTP
// codes are accepted once each, so a second sign-in waits for an unused code.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { NEWCOMER_SESSION, PASSWORD, WEB_PORT, call, currentStep, freshTotp, instant, iso, now, recordStep, setClock, state, until } from "./stack";

test.describe.configure({ mode: "serial" });

const NEWCOMER = `newcomer${Date.now() % 100000}`;
const CPI = 101;
let newcomerSecret = "";

async function accessible(page: Page, what: string) {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
  const summary = results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
  expect(summary, `${what} has accessibility violations`).toEqual([]);
}

async function signIn(page: Page, username: string, secret: string) {
  await page.goto("/signin");
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Log in", exact: true }).click();
  await page.getByLabel("Authenticator code").fill(await freshTotp(username, secret));
  await page.getByRole("button", { name: "Log in", exact: true }).click();
  await expect(page.getByRole("link", { name: username })).toBeVisible();
}

/** A page signed in as the newcomer, from the session saved by the first test. */
async function asNewcomer(browser: Browser): Promise<Page> {
  const context = await browser.newContext({ storageState: NEWCOMER_SESSION, baseURL: `http://127.0.0.1:${WEB_PORT}` });
  return context.newPage();
}

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
  recordStep(NEWCOMER, currentStep() - 2);
  await page.getByLabel("6-digit code").fill(await freshTotp(NEWCOMER, newcomerSecret));
  await page.getByRole("button", { name: "Turn on", exact: true }).click();
  await expect(page.getByText("Two-step sign-in is on.")).toBeVisible();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("link", { name: "Log in" })).toBeVisible();
  await page.goto("/signin");
  await accessible(page, "sign-in");
  await signIn(page, NEWCOMER, newcomerSecret);
  await page.context().storageState({ path: NEWCOMER_SESSION });
});

test("the market list shows the launch catalogue, open by schedule", async ({ page }) => {
  await until("the scheduler to open the markets", async () => {
    const { markets } = await call<{ markets: { market: number; state: string }[] }>("GET", "/v1/markets");
    const catalogue = state().markets.map((m) => m.market);
    return catalogue.every((id) => markets.some((m) => m.market === id && m.state === "open"));
  });
  await page.goto("/");
  await accessible(page, "market list");
  for (const m of state().markets) await expect(page.getByRole("article", { name: m.title })).toBeVisible();
});

test("a trade: buy Yes from the panel, fill against another trader, and cancel a resting order", async ({ browser }) => {
  const page = await asNewcomer(browser);
  await page.goto(`/markets/${CPI}`);
  await expect(page.getByText("● Live")).toBeVisible();
  await accessible(page, "market page");
  // Buy 10 Yes at 55: it rests, and the live book shows the bid.
  await page.getByRole("spinbutton", { name: "Contracts" }).fill("10");
  await page.getByRole("spinbutton", { name: /Limit price/ }).fill("55");
  await page.getByRole("button", { name: "Buy 10 Yes at 55¢" }).click();
  await expect(page.getByText(/0 of 10 filled; 10 resting/)).toBeVisible();
  await expect(page.getByRole("table", { name: "Bids to buy YES" }).getByRole("cell", { name: "55¢" })).toBeVisible();
  // Tom sells 10 Yes at 55 (buys No at 45): the order fills.
  await call("POST", "/v1/orders", state().users.tom!.token, { market: CPI, side: "sell", price: 55, qty: 10 });
  await page.goto("/portfolio");
  await accessible(page, "portfolio");
  await expect(page.getByRole("cell", { name: "10 Yes" })).toBeVisible();
  // A resting No order, then cancelled from the portfolio.
  await page.goto(`/markets/${CPI}`);
  await page.getByRole("group", { name: "Outcome" }).getByRole("button", { name: /^No/ }).click();
  await page.getByRole("spinbutton", { name: "Contracts" }).fill("3");
  await page.getByRole("spinbutton", { name: /Limit price/ }).fill("20");
  await page.getByRole("button", { name: "Buy 3 No at 20¢" }).click();
  await expect(page.getByText(/3 resting/)).toBeVisible();
  await page.goto("/portfolio");
  await expect(page.getByRole("cell", { name: "Buy No at 20¢" })).toBeVisible();
  await page.getByRole("button", { name: /Cancel order/ }).click();
  await expect(page.getByText("No open orders.")).toBeVisible();
});

test("an event: browse its category, open it, and buy one outcome by amount", async ({ browser }) => {
  const { event, users } = state();
  const range = event.markets.find((m) => m.outcome === "59-60°F")!;
  await until("the scheduler to open the event", async () => {
    const e = await call<{ status: string }>("GET", `/v1/events/${event.slug}`);
    return e.status === "open";
  });
  // Another trader offers 20 contracts of one range at 30¢.
  await call("POST", "/v1/orders", users.tom!.token, { market: range.market, side: "sell", price: 30, qty: 20 });
  const page = await asNewcomer(browser);
  await page.goto("/c/weather");
  await accessible(page, "weather category");
  await page.getByRole("link", { name: event.title }).click();
  await expect(page.getByRole("heading", { level: 1, name: event.title })).toBeVisible();
  await accessible(page, "event page");
  await page.getByRole("button", { name: /^59-60°F/ }).click();
  await expect(page.getByText("Live", { exact: true })).toBeVisible();
  await page.getByRole("spinbutton", { name: "Amount" }).fill("3");
  // 3 credits buy 10 contracts at 30¢, paying 10 credits if the range is right.
  await expect(page.getByText("10.00")).toBeVisible();
  await page.getByRole("button", { name: "Buy Yes · 3.00" }).click();
  await expect(page.getByText("Bought 10 Yes on “59-60°F”.")).toBeVisible();
  await page.goto("/portfolio");
  await expect(page.getByRole("row", { name: /New York high 59-60°F on Nov 12.*10 Yes/ })).toBeVisible();
  for (const [path, what] of [["/movers", "movers"], ["/new", "new markets"], ["/search?q=new+york", "search"], ["/watchlist", "watchlist"]] as const) {
    await page.goto(path);
    await accessible(page, what);
  }
});

test("a forecast is recorded, and covered by the next price samples", async ({ browser }) => {
  const page = await asNewcomer(browser);
  await page.goto(`/markets/${CPI}`);
  await page.getByLabel("Your probability of YES").fill("70");
  await page.getByRole("button", { name: /Save forecast|Update forecast/ }).click();
  await expect(page.getByText("Recorded: 70%.")).toBeVisible();
  for (let i = 0; i < 2; i++) {
    setClock(now() + 60);
    await new Promise((r) => setTimeout(r, 1500));
  }
});

test("resolution: an admin confirms in the console, a holder disputes, the dispute is rejected, and the market settles", async ({ browser, page }) => {
  const s = state();
  const cpi = s.markets.find((m) => m.market === CPI)!;
  setClock(instant(cpi.release) + 60);
  await until("market 101 to close", async () => {
    const { markets } = await call<{ markets: { market: number; state: string }[] }>("GET", "/v1/markets");
    return markets.find((m) => m.market === CPI)?.state === "closed";
  });
  // Ada proposes Yes (proposing is the resolver service's job, tested in Python).
  const evidence = { market: CPI, rules_hash: cpi.rules_hash, outcome: "yes", retrieved_at: iso(now()), source: "bls", value: "3.7" };
  await call("POST", `/v1/markets/${CPI}/resolution`, s.users.ada!.token, { outcome: "yes", evidence, reason: "BLS published November CPI" });

  // Cyd confirms it in the admin console.
  await signIn(page, "cyd", s.users.cyd!.secret!);
  await page.goto("/admin");
  await accessible(page, "admin console");
  await page.getByRole("textbox", { name: /Reason \(3/ }).fill("checked against the BLS release");
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.getByText(/Confirm resolution \d+: done/)).toBeVisible();

  // The newcomer, who holds Yes, disputes on the market page.
  const holder = await asNewcomer(browser);
  await holder.goto(`/markets/${CPI}`);
  await holder.getByLabel(/Think this outcome is wrong/).fill("I think the release was revised");
  await holder.getByRole("button", { name: "File a dispute" }).click();
  await expect(holder.getByText(/Dispute filed/)).toBeVisible();

  // Cyd rejects the dispute.
  await page.goto("/admin");
  await expect(page.getByRole("region", { name: "Open disputes" }).getByRole("cell", { name: "I think the release was revised" })).toBeVisible();
  await page.getByRole("textbox", { name: /Reason \(3/ }).fill("the rulebook counts the first release");
  await page.getByRole("button", { name: "Reject" }).click();
  await expect(page.getByText(/Reject dispute \d+: done/)).toBeVisible();

  // The window ends; the gateway settles.
  setClock(now() + 3_601);
  await until("settlement", async () => {
    const view = await call<{ resolutions: { state: string }[]; journal: unknown }>("GET", `/v1/markets/${CPI}/resolution`);
    return view.resolutions[0]?.state === "settled" && view.journal !== null;
  });
  await holder.goto(`/markets/${CPI}`);
  await expect(holder.getByText("settled", { exact: true })).toBeVisible();
  await holder.getByText("Evidence record").first().click();
  await expect(holder.getByText("Matches the exchange log")).toBeVisible();
});

test("the scorecard and leaderboards show the result", async ({ browser }) => {
  const page = await asNewcomer(browser);
  const title = state().markets.find((m) => m.market === CPI)!.title;
  // The scorer runs on the gateway's next pass after the settlement reaches the read models.
  await expect(async () => {
    await page.goto("/scorecard");
    await expect(page.getByRole("link", { name: title, exact: true })).toBeVisible({ timeout: 1_000 });
  }).toPass({ timeout: 20_000 });
  await accessible(page, "scorecard");
  await page.goto("/leaderboards");
  await accessible(page, "leaderboards");
  // Bought 10 Yes at 55, Yes won: +4.50 credits.
  await expect(page.getByRole("row", { name: new RegExp(`${NEWCOMER}.*\\+4\\.50`) })).toBeVisible();
});
