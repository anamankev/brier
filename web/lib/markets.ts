// Server-side summaries of every listed market: the numbers a market card or row shows, all
// read from the gateway (nothing is invented when a market has no trades yet).
import { gateway } from "./server";
import type { MarketRow, Rules } from "./types";

export type Summary = MarketRow & {
  title: string;
  /** The market's probability: the last trade; before any trade, the book midpoint. */
  chance: number | null;
  /** What it costs to buy one contract now: the best YES offer, and 100 minus the best YES bid. */
  yesPrice: number | null;
  noPrice: number | null;
  /** Change in percentage points over the last 24 hours of trading, from the minute prices. */
  change: number | null;
  points: [string, number][];
  release: string;
  rulesPinned: boolean;
};

type Book = { bids: [number, number][]; asks: [number, number][] };

function dayChange(points: [string, number][]): number | null {
  if (points.length < 2) return null;
  const last = points[points.length - 1]!;
  const since = Date.parse(last[0]) - 24 * 3600 * 1000;
  const base = [...points].reverse().find((p) => Date.parse(p[0]) <= since) ?? points[0]!;
  return last[1] - base[1];
}

export async function summaries(): Promise<Summary[]> {
  const list = await gateway<{ markets: MarketRow[] }>("/v1/markets");
  if (!list.ok) return [];
  const listed = list.body.markets.filter((m): m is MarketRow & { title: string } => Boolean(m.title));
  return Promise.all(
    listed.map(async (m) => {
      const [book, prices, rules] = await Promise.all([
        gateway<Book>(`/v1/markets/${m.market}/book`),
        gateway<{ points: [string, number][] }>(`/v1/markets/${m.market}/prices`),
        gateway<Rules>(`/v1/markets/${m.market}/rules`),
      ]);
      const bid = book.ok ? book.body.bids[0]?.[0] : undefined;
      const ask = book.ok ? book.body.asks[0]?.[0] : undefined;
      const mid = bid !== undefined && ask !== undefined ? Math.round((bid + ask) / 2) : null;
      const points = prices.ok ? prices.body.points : [];
      return {
        ...m,
        chance: m.last_price ?? mid,
        yesPrice: ask ?? null,
        noPrice: bid !== undefined ? 100 - bid : null,
        change: dayChange(points),
        points,
        release: rules.ok ? rules.body.schedule.release : (m.close ?? ""),
        rulesPinned: rules.ok && Boolean(rules.body.journal?.matches),
      };
    }),
  );
}
