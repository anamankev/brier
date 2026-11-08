"use client";
import { useMarketData } from "@/lib/book";
import { Ladder } from "./Ladder";
import { PriceChart } from "./PriceChart";

type Trade = { seq: number; price: number; qty: number; taker_side: string };

export function MarketLive({ market, points, recent }: { market: number; points: [string, number][]; recent: Trade[] }) {
  const { book, trades, connected } = useMarketData(market);
  const now = new Date().toISOString();
  const live: [string, number][] = trades
    .slice()
    .reverse()
    .map((t) => [new Date(t.ts / 1000).toISOString(), t.price]);
  const series = [...points, ...live];
  if (series.length === 1) series.push([now, series[0]![1]]);
  const shownTrades = [
    ...trades.map((t) => ({ key: `l${t.seq}`, price: t.price, qty: t.qty, side: t.taker_side })),
    ...recent.map((t) => ({ key: `r${t.seq}-${t.price}`, price: t.price, qty: t.qty, side: t.taker_side })),
  ].slice(0, 12);
  return (
    <>
      <section className="card chart" aria-label="Price history">
        <h2>Price</h2>
        <PriceChart points={series} />
      </section>
      <div className="grid market-grid">
        <section className="card">
          <Ladder book={book} />
          <p className="muted" role="status" style={{ fontSize: "0.8rem", margin: "8px 0 0" }}>
            {connected ? "Live" : "Reconnecting to market data…"}
          </p>
        </section>
        <section className="card">
          <table>
            <caption>Recent trades</caption>
            <thead>
              <tr>
                <th scope="col">Taker</th>
                <th scope="col" className="num">
                  YES price
                </th>
                <th scope="col" className="num">
                  Contracts
                </th>
              </tr>
            </thead>
            <tbody>
              {shownTrades.map((t) => (
                <tr key={t.key}>
                  <td className={t.side === "buy" ? "yes" : "no"}>{t.side === "buy" ? "Bought YES" : "Bought NO"}</td>
                  <td className="num">{t.price}¢</td>
                  <td className="num">{t.qty}</td>
                </tr>
              ))}
              {shownTrades.length === 0 && (
                <tr>
                  <td colSpan={3} className="muted">
                    No trades yet.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </section>
      </div>
    </>
  );
}
