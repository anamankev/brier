import type { MetadataRoute } from "next";
import { CATEGORIES } from "@/lib/events";
import { listEvents } from "@/lib/events-server";
import { SITE_URL } from "@/lib/site";

export const dynamic = "force-dynamic";

export default async function sitemap(): Promise<MetadataRoute.Sitemap> {
  const open = await listEvents({ status: "open", sort: "volume", limit: 200 });
  const fixed = ["", "/movers", "/new", "/leaderboards", "/rules", "/terms", "/privacy", "/credits"].map((p) => ({ url: `${SITE_URL}${p}` }));
  const categories = CATEGORIES.map((c) => ({ url: `${SITE_URL}/c/${c.slug}`, changeFrequency: "hourly" as const }));
  const events = (open?.events ?? []).map((e) => ({ url: `${SITE_URL}/e/${e.slug}`, changeFrequency: "hourly" as const }));
  return [...fixed, ...categories, ...events];
}
