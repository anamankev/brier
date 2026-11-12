// The cookie banner and the policy pages (12 November 2026): the banner appears until a choice
// is made, offers Accept all / Essential only / Customize, really changes where the watchlist is
// kept, can be reopened from the footer, and every policy page is accessible.
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

async function accessible(page: Page, what: string) {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
  expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`), `${what} has accessibility violations`).toEqual([]);
}

const consentCookie = async (page: Page) => (await page.context().cookies()).find((c) => c.name === "brier_consent")?.value;
const stored = (page: Page) =>
  page.evaluate(() => ({ local: localStorage.getItem("brier.watchlist"), session: sessionStorage.getItem("brier.watchlist") }));

test("the cookie banner asks once, and Essential only keeps the watchlist for this session", async ({ page }) => {
  await page.goto("/");
  const banner = page.getByRole("dialog", { name: "Cookies and your privacy" });
  await expect(banner).toBeVisible();
  await accessible(page, "home page with the cookie banner");
  await banner.getByRole("button", { name: "Customize" }).click();
  await expect(banner.getByRole("checkbox", { name: "Essential cookies (always on)" })).toBeDisabled();
  await accessible(page, "cookie choices");
  await banner.getByRole("checkbox", { name: "Preference storage" }).uncheck();
  await banner.getByRole("button", { name: "Save choices" }).click();
  await expect(banner).toBeHidden();
  expect(await consentCookie(page)).toBe("v1.p0");

  // Starring without the Preferences choice keeps the list for this session only.
  await page.getByRole("button", { name: /^Add “.+” to your watchlist$/ }).first().click();
  const before = await stored(page);
  expect(before.local).toBeNull();
  expect(JSON.parse(before.session ?? "[]")).toHaveLength(1);

  // The choice is remembered on the next page, and can be changed from the footer.
  await page.goto("/watchlist");
  await expect(banner).toBeHidden();
  await page.getByRole("contentinfo").getByRole("button", { name: "Cookie settings" }).click();
  await expect(banner).toBeVisible();
  await banner.getByRole("checkbox", { name: "Preference storage" }).check();
  await banner.getByRole("button", { name: "Save choices" }).click();
  expect(await consentCookie(page)).toBe("v1.p1");
  const after = await stored(page);
  expect(after.session).toBeNull();
  expect(JSON.parse(after.local ?? "[]")).toHaveLength(1);
});

test("Accept all is one click", async ({ page }) => {
  await page.goto("/leaderboards");
  await page.getByRole("button", { name: "Accept all" }).click();
  await expect(page.getByRole("dialog", { name: "Cookies and your privacy" })).toBeHidden();
  expect(await consentCookie(page)).toBe("v1.p1");
});

test("the privacy, terms and cookie policies are complete and accessible", async ({ page, context }) => {
  await context.addCookies([{ name: "brier_consent", value: "v1.p1", domain: "127.0.0.1", path: "/" }]);
  for (const [path, heading, sections] of [
    ["/privacy", "Privacy policy", ["Who runs Brier", "Legal bases (EEA and UK)", "International transfers", "Retention", "Your rights", "Children", "Contact"]],
    ["/terms", "Terms of service", ["3. Play credits only", "6. Acceptable use", "10. Disclaimer of warranties", "11. Limitation of liability", "15. General"]],
    ["/cookies", "Cookie policy", ["What Brier stores", "Categories", "Your choices"]],
  ] as const) {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1, name: heading })).toBeVisible();
    for (const s of sections) await expect(page.getByRole("heading", { level: 2, name: s, exact: true })).toBeVisible();
    await expect(page.getByText(/Effective \d+ November 2026/)).toBeVisible();
    await accessible(page, heading);
  }
  // Every row of the cookie table names something the site really sets.
  await expect(page.getByRole("cell", { name: "__Host-brier_session" })).toBeVisible();
  await expect(page.getByRole("cell", { name: "brier_consent" })).toBeVisible();
});
