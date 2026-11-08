"use client";
import type { Book } from "@/lib/book";

/** The order book as a ladder: asks (offers to sell YES) above, bids below, best prices at the
 * spread. Each row also says what it means for a NO buyer, so colour is never the only cue. */
export function Ladder({ book, depth = 8 }: { book: Book; depth?: number }) {
  const asks = [...book.asks.entries()].sort((a, b) => a[0] - b[0]).slice(0, depth).reverse();
  const bids = [...book.bids.entries()].sort((a, b) => b[0] - a[0]).slice(0, depth);
  const max = Math.max(1, ...asks.map((l) => l[1]), ...bids.map((l) => l[1]));
  const bestBid = bids[0]?.[0];
  const bestAsk = asks[asks.length - 1]?.[0];
  return (
    <table className="ladder" aria-describedby="ladder-help">
      <caption>
        Order book{" "}
        {!book.synced && (
          <span className="muted" role="status">
            (syncing…)
          </span>
        )}
      </caption>
      <thead>
        <tr>
          <th scope="col">Side</th>
          <th scope="col" className="num">
            YES price
          </th>
          <th scope="col" className="num">
            Contracts
          </th>
        </tr>
      </thead>
      <tbody>
        {asks.map(([price, qty]) => (
          <tr key={`a${price}`} className="ask">
            <td className="no">Sell YES</td>
            <td className="num">{price}¢</td>
            <td className="num bar">
              <span style={{ width: `${(qty / max) * 100}%` }} />
              {qty.toLocaleString("en-US")}
            </td>
          </tr>
        ))}
        <tr className="spread">
          <td colSpan={3}>
            {bestBid !== undefined && bestAsk !== undefined
              ? `Spread ${bestAsk - bestBid}¢ · midpoint ${((bestBid + bestAsk) / 2).toFixed(1)}%`
              : "No two-sided market yet"}
          </td>
        </tr>
        {bids.map(([price, qty]) => (
          <tr key={`b${price}`} className="bid">
            <td className="yes">Buy YES</td>
            <td className="num">{price}¢</td>
            <td className="num bar">
              <span style={{ width: `${(qty / max) * 100}%` }} />
              {qty.toLocaleString("en-US")}
            </td>
          </tr>
        ))}
      </tbody>
      <tfoot>
        <tr>
          <td colSpan={3} id="ladder-help" className="muted" style={{ fontSize: "0.82rem" }}>
            Buying NO at q is the same as selling YES at 100 − q, so one book serves both.
          </td>
        </tr>
      </tfoot>
    </table>
  );
}
