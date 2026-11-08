import Link from "next/link";
import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { credits } from "@/lib/format";
import type { MarketRow, Portfolio } from "@/lib/types";
import { CancelButton } from "./CancelButton";

export const dynamic = "force-dynamic";

type Fill = { seq: number; market: number; price: number; qty: number; side: string; role?: string };

export default async function PortfolioPage() {
  const user = await me();
  if (!user) redirect("/signin");
  const [p, list, fills] = await Promise.all([
    gateway<Portfolio>("/v1/portfolio"),
    gateway<{ markets: MarketRow[] }>("/v1/markets"),
    gateway<{ fills: Fill[] }>("/v1/fills?limit=50"),
  ]);
  if (!p.ok) return <p className="notice error">Could not load the portfolio.</p>;
  const markets = new Map((list.ok ? list.body.markets : []).map((m) => [m.market, m]));
  const title = (id: number) => markets.get(id)?.title ?? `Market ${id}`;
  const value = (id: number, x: number) => {
    const last = markets.get(id)?.last_price;
    if (last == null) return null;
    return x > 0 ? x * last : -x * (100 - last);
  };
  return (
    <>
      <h1>Portfolio</h1>
      <section className="card">
        <table>
          <tbody>
            <tr>
              <th scope="row">Cash</th>
              <td className="num">{credits(p.body.cash)}</td>
            </tr>
            <tr>
              <th scope="row">Reserved by open orders</th>
              <td className="num">{credits(p.body.reserved)}</td>
            </tr>
            <tr>
              <th scope="row">Available</th>
              <td className="num">{credits(p.body.available)}</td>
            </tr>
          </tbody>
        </table>
      </section>
      <section className="card">
        <table>
          <caption>Positions</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Holding</th>
              <th scope="col" className="num">
                Value at last trade
              </th>
            </tr>
          </thead>
          <tbody>
            {p.body.positions.map((pos) => {
              const v = value(pos.market, pos.position);
              return (
                <tr key={pos.market}>
                  <td>
                    <Link href={`/markets/${pos.market}`}>{title(pos.market)}</Link>
                  </td>
                  <td className={pos.position > 0 ? "yes" : "no"}>
                    {Math.abs(pos.position)} {pos.position > 0 ? "YES" : "NO"}
                  </td>
                  <td className="num">{v == null ? "—" : credits(v)}</td>
                </tr>
              );
            })}
            {p.body.positions.length === 0 && (
              <tr>
                <td colSpan={3} className="muted">
                  No positions.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
      <section className="card">
        <table>
          <caption>Open orders</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Order</th>
              <th scope="col" className="num">
                Remaining
              </th>
              <th scope="col">
                <span className="skip">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {p.body.open_orders.map((o) => (
              <tr key={o.order}>
                <td>{title(o.market)}</td>
                <td className={o.side === "buy" ? "yes" : "no"}>
                  {o.side === "buy" ? `Buy YES at ${o.price}¢` : `Buy NO at ${100 - o.price}¢`}
                </td>
                <td className="num">{o.remaining}</td>
                <td>
                  <CancelButton order={o.order} />
                </td>
              </tr>
            ))}
            {p.body.open_orders.length === 0 && (
              <tr>
                <td colSpan={4} className="muted">
                  No open orders.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
      <section className="card">
        <table>
          <caption>Recent fills</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col" className="num">
                YES price
              </th>
              <th scope="col" className="num">
                Contracts
              </th>
            </tr>
          </thead>
          <tbody>
            {(fills.ok ? fills.body.fills : []).map((f) => (
              <tr key={`${f.seq}-${f.price}-${f.qty}`}>
                <td>{title(f.market)}</td>
                <td className="num">{f.price}¢</td>
                <td className="num">{f.qty}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  );
}
