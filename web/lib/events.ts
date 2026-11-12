// Events: markets grouped as the outcomes of one question (GET /v1/events). A market listed on
// its own arrives as an event of one market, so every page reads one shape.

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

export const CATEGORIES: { slug: string; name: string; blurb: string }[] = [
  { slug: "weather", name: "Weather", blurb: "Official daily highs in twelve US cities, from the National Weather Service." },
  { slug: "science", name: "Science", blurb: "Earthquakes worldwide from the USGS catalog, and geomagnetic storms from NOAA." },
  { slug: "economics", name: "Economy", blurb: "Inflation and jobs, from the Bureau of Labor Statistics." },
  { slug: "rates", name: "Rates", blurb: "The Fed's target rate and Treasury yields." },
  { slug: "sports", name: "Sports", blurb: "Every NBA and NFL game, and five European soccer leagues, until kickoff." },
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

/** How a tag reads in a sidebar; tags that only describe the format are not listed. */
export const TAG_NAMES: Record<string, string> = {
  nyc: "New York",
  chicago: "Chicago",
  "los-angeles": "Los Angeles",
  miami: "Miami",
  boston: "Boston",
  seattle: "Seattle",
  denver: "Denver",
  atlanta: "Atlanta",
  phoenix: "Phoenix",
  "washington-dc": "Washington, DC",
  austin: "Austin",
  dallas: "Dallas",
  earthquakes: "Earthquakes",
  "space-weather": "Space weather",
  inflation: "Inflation",
  jobs: "Jobs",
  fed: "Fed decisions",
  treasury: "Treasury yields",
  nba: "NBA",
  nfl: "NFL",
  "premier-league": "Premier League",
  "la-liga": "La Liga",
  "serie-a": "Serie A",
  bundesliga: "Bundesliga",
  "ligue-1": "Ligue 1",
};

/** Sports topics: the league tags shown as subcategories (the sport tags only pick photos). */
const LEAGUE_BLURBS: Record<string, string> = {
  nba: "Every NBA game, with a market for each team, until tip-off. Results include overtime.",
  nfl: "Every NFL game, with a market for each team, until kickoff. Results include overtime; a tie resolves both No.",
};

/** What a topic page under a category covers. */
const ECONOMY_BLURBS: Record<string, string> = {
  inflation: "The next CPI report's 12-month change, in ranges, settled from the BLS release.",
  jobs: "The next jobs report's unemployment rate, settled from the BLS release.",
  fed: "What the Fed does at its next meeting, settled from the target range the next day.",
  treasury: "The 10-year Treasury yield at the end of next week, from the Fed's H.15 via FRED.",
};

export function tagBlurb(tag: string): string | null {
  if (ECONOMY_BLURBS[tag]) return ECONOMY_BLURBS[tag]!;
  if (LEAGUE_BLURBS[tag]) return LEAGUE_BLURBS[tag]!;
  if (["premier-league", "la-liga", "serie-a", "bundesliga", "ligue-1"].includes(tag))
    return `Every ${TAG_NAMES[tag]} match: home win, draw or away win after 90 minutes, until kickoff.`;
  if (tag === "earthquakes") return "Daily counts of magnitude 5+ and 6+ earthquakes worldwide, from the USGS catalog.";
  if (tag === "space-weather") return "The day's strongest geomagnetic storm, from NOAA's Space Weather Prediction Center.";
  const city = TAG_NAMES[tag];
  return city ? `The official daily high in ${city}, from the National Weather Service.` : null;
}

export const SORTS: { slug: NonNullable<EventQuery["sort"]>; name: string }[] = [
  { slug: "volume_24h", name: "Trending" },
  { slug: "volume", name: "Volume" },
  { slug: "closing", name: "Closing soon" },
  { slug: "newest", name: "New" },
  { slug: "movers", name: "Movers" },
];
