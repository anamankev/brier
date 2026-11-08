// Market orders by amount: what a budget buys against the book right now, and the single
// immediate-or-cancel order that buys it. The engine has no market orders; an IOC limit at the
// worst price the plan reaches fills exactly the planned levels, or less if the book moved.
import type { Book } from "./book";

export type Plan = {
  /** The order to send: a side of the YES book, a limit price, and contracts. */
  side: "buy" | "sell";
  price: number;
  qty: number;
  /** Credits (cents) spent or received, at the levels in the book now. */
  total: number;
  /** Average price per contract, in cents of the chosen outcome (YES or NO). */
  average: number;
};

/** Spend at most `budget` cents on YES or NO, guaranteed even if the book moves. Buying YES
 * takes offers, cheapest first; buying NO sells YES into bids, highest first, at a cost of 100
 * minus the bid. */
export function planBuy(book: Book, outcome: "yes" | "no", budget: number): Plan | null {
  const levels =
    outcome === "yes"
      ? [...book.asks.entries()].sort((a, b) => a[0] - b[0]).map(([p, q]) => [p, p, q] as const)
      : [...book.bids.entries()].sort((a, b) => b[0] - a[0]).map(([p, q]) => [p, 100 - p, q] as const);
  let left = budget;
  let qty = 0;
  let total = 0;
  let worst: number | null = null;
  for (const [bookPrice, cost, size] of levels) {
    const n = Math.min(size, Math.floor(left / cost));
    if (n <= 0) break;
    qty += n;
    total += n * cost;
    left -= n * cost;
    worst = bookPrice;
    if (n < size) break;
  }
  if (worst === null || qty === 0) return null;
  // The order is one IOC at the worst price reached. If the book changes before it arrives,
  // every contract could fill at that price, so cap the quantity to keep that within budget.
  const worstCost = outcome === "yes" ? worst : 100 - worst;
  const cap = Math.floor(budget / worstCost);
  if (qty > cap) {
    qty = cap;
    total = 0;
    let need = qty;
    for (const [, cost, size] of levels) {
      const n = Math.min(size, need);
      total += n * cost;
      need -= n;
      if (need === 0) break;
    }
  }
  if (qty === 0) return null;
  return { side: outcome === "yes" ? "buy" : "sell", price: worst, qty, total, average: total / qty };
}

/** Sell `contracts` of a held YES (into bids) or NO (buying YES back from offers). */
export function planSell(book: Book, outcome: "yes" | "no", contracts: number): Plan | null {
  const levels =
    outcome === "yes"
      ? [...book.bids.entries()].sort((a, b) => b[0] - a[0]).map(([p, q]) => [p, p, q] as const)
      : [...book.asks.entries()].sort((a, b) => a[0] - b[0]).map(([p, q]) => [p, 100 - p, q] as const);
  let left = contracts;
  let qty = 0;
  let total = 0;
  let worst: number | null = null;
  for (const [bookPrice, proceeds, size] of levels) {
    const n = Math.min(size, left);
    if (n <= 0) break;
    qty += n;
    total += n * proceeds;
    left -= n;
    worst = bookPrice;
  }
  if (worst === null || qty === 0) return null;
  return { side: outcome === "yes" ? "sell" : "buy", price: worst, qty, total, average: total / qty };
}
