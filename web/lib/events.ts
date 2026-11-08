// Events: markets grouped as the outcomes of one question (GET /v1/events). A market listed on
// its own arrives as an event of one market, so every page reads one shape.
import { gateway } from "./server";

export type EventMarket = {
  market: number;
  title: string;
  outcome: string | null;
  state: string | null;
  /** The market's probability in percent: the last trade, else the middle of the book. */
  chance: number | null;
  change_24h: number | null;
  last_price: number | null;
  best_bid: number | null;
  best_ask: number | null;
  volume: number;
  volume_24h: number;
};

export type EventSummary = {
  slug: string;
  title: string;
  category: string | null;
  tags: string[];
  series: string | null;
  exclusive: boolean;
  standalone: boolean;
  observation: string;
  close: string;
  release: string;
  status: "open" | "upcoming" | "closed" | "settled";
  volume: number;
  volume_24h: number;
  markets: EventMarket[];
  siblings?: { slug: string; observation: string; status: string }[];
};

export type EventList = { events: EventSummary[]; categories: Record<string, number> };

export type EventQuery = {
  category?: string;
  tag?: string;
  series?: string;
  status?: "open" | "upcoming" | "closed" | "settled" | "all";
  sort?: "volume" | "volume_24h" | "movers" | "newest" | "closing";
  q?: string;
  limit?: number;
};

export async function listEvents(query: EventQuery = {}): Promise<EventList | null> {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) if (v !== undefined && v !== "") params.set(k, String(v));
  const res = await gateway<EventList>(`/v1/events?${params}`);
  return res.ok ? res.body : null;
}

export async function getEvent(slug: string): Promise<EventSummary | null> {
  const res = await gateway<EventSummary>(`/v1/events/${encodeURIComponent(slug)}`);
  return res.ok ? res.body : null;
}

/** Outcomes most likely first, for cards that show the top two. */
export function leading(e: EventSummary): EventMarket[] {
  return [...e.markets].sort((a, b) => (b.chance ?? -1) - (a.chance ?? -1));
}

/** What to call the event's outcome: its label, or "Yes" for a single question. */
export function label(m: EventMarket): string {
  return m.outcome ?? "Yes";
}

/** Buy YES at the best offer; buy NO at 100 minus the best bid (the single YES book). */
export function yesPrice(m: EventMarket): number | null {
  return m.best_ask;
}
export function noPrice(m: EventMarket): number | null {
  return m.best_bid == null ? null : 100 - m.best_bid;
}

/** The kind of picture an event gets, from its category and tags. */
export function kind(e: Pick<EventSummary, "category" | "tags">): string {
  if (e.tags.includes("earthquakes")) return "earthquakes";
  if (e.tags.includes("space-weather")) return "space";
  return e.category ?? "economics";
}

export const CATEGORIES: { slug: string; name: string; blurb: string }[] = [
  { slug: "weather", name: "Weather", blurb: "Official daily highs in twelve US cities, from the National Weather Service." },
  { slug: "science", name: "Science", blurb: "Earthquakes worldwide from the USGS catalog, and geomagnetic storms from NOAA." },
  { slug: "economics", name: "Economy", blurb: "Inflation and jobs, from the Bureau of Labor Statistics." },
  { slug: "rates", name: "Rates", blurb: "The Fed's target rate and Treasury yields." },
];

export function categoryName(slug: string | null | undefined): string {
  return CATEGORIES.find((c) => c.slug === slug)?.name ?? "Markets";
}

/** "Daily", "Monthly", or nothing, from the series' name. */
export function cadence(e: Pick<EventSummary, "series" | "tags">): string | null {
  if (e.tags.includes("daily") || e.series?.endsWith("-daily") || e.series?.endsWith("-daily-high")) return "Daily";
  return null;
}

/** "1.2K", "34K", "1.4M" contracts. */
export function compact(n: number): string {
  if (n < 1000) return n.toLocaleString("en-US");
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
