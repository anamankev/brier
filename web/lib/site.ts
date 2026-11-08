/** The public address the site is served at, for absolute links in previews, the sitemap and
 * robots.txt. Set BRIER_SITE_URL when deploying. */
export const SITE_URL = (process.env.BRIER_SITE_URL ?? "http://localhost:3005").replace(/\/$/, "");
