"use client";
import type { Book } from "@/lib/book";

/** The book in two columns: bids to buy YES (each also an offer to sell NO) and offers to sell
 * YES. Depth bars show size; the words carry the meaning. */
export function OrderBook({ book, depth = 8 }: { book: Book; depth?: number }) {
  const bids = [...book.bids.entries()].sort((a, b) => b[0] - a[0]).slice(0, depth);
  const asks = [...book.asks.entries()].sort((a, b) => a[0] - b[0]).slice(0, depth);
  const max = Math.max(1, ...bids.map((l) => l[1]), ...asks.map((l) => l[1]));
  const side = (levels: [number, number][], cls: "bids" | "asks", label: string) => (
    <table className={cls}>
      <caption className="sr-only">{label}</caption>
      <thead>
        <tr>
          <th scope="col">{cls === "bids" ? "Bid (Yes)" : "Ask (Yes)"}</th>
          <th scope="col" className="num">
            Contracts
          </th>
        </tr>
      </thead>
      <tbody>
        {levels.map(([price, qty]) => (
          <tr key={price}>
            <td className={cls === "bids" ? "yes" : "no"}>
              <span className="v">{price}¢</span>
            </td>
            <td className="num">
              <span className="depth" style={{ width: `${(qty / max) * 100}%` }} />
              <span className="v">{qty.toLocaleString("en-US")}</span>
            </td>
          </tr>
        ))}
        {levels.length === 0 && (
          <tr>
            <td colSpan={2} className="muted">
              {cls === "bids" ? "No bids" : "No offers"}
            </td>
          </tr>
        )}
      </tbody>
    </table>
  );
  const bestBid = bids[0]?.[0];
  const bestAsk = asks[0]?.[0];
  return (
    <div>
      <div className="spread" style={{ padding: "16px 16px 8px" }}>
        <h2 style={{ fontSize: 16 }}>Order book</h2>
        <span className="muted" style={{ fontSize: 13 }} role="status">
          {!book.synced ? "Syncing…" : bestBid !== undefined && bestAsk !== undefined ? `Spread ${bestAsk - bestBid}¢` : "One-sided"}
        </span>
      </div>
      <div className="book">
        {side(bids, "bids", "Bids to buy YES")}
        {side(asks, "asks", "Offers to sell YES")}
      </div>
      <p className="muted" style={{ fontSize: 12.5, padding: "8px 16px 16px", margin: 0 }}>
        Buying NO at q¢ is the same order as selling YES at (100 − q)¢, so one book serves both sides.
      </p>
    </div>
  );
}
