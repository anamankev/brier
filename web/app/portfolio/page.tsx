import Link from "next/link";
import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { summaries } from "@/lib/markets";
import { chance, credits } from "@/lib/format";
import type { Portfolio } from "@/lib/types";
import { CategoryTile } from "@/components/Icons";
import { CancelButton } from "./CancelButton";

export const dynamic = "force-dynamic";
export const metadata = { title: "Portfolio" };

type Fill = { seq: number; market: number; price: number; qty: number; taker_side: string; side?: string };

export default async function PortfolioPage() {
  const user = await me();
  if (!user) redirect("/signin");
  const [p, all, fills] = await Promise.all([gateway<Portfolio>("/v1/portfolio"), summaries(), gateway<{ fills: Fill[] }>("/v1/fills?limit=50")]);
  if (!p.ok) return <p className="notice error">Could not load the portfolio.</p>;
  const byId = new Map(all.map((m) => [m.market, m]));
  // Marked at the market's chance: YES at the chance, NO at 100 minus it.
  const value = (id: number, x: number) => {
    const c = byId.get(id)?.chance;
    return c == null ? null : x > 0 ? x * c : -x * (100 - c);
  };
  const marked = p.body.positions.reduce((sum, pos) => sum + (value(pos.market, pos.position) ?? 0), 0);
  return (
    <div className="stack" style={{ gap: 24 }}>
      <h1 className="display">Portfolio</h1>
      <div className="stats">
        <div className="card stat">
          <div className="label">Cash</div>
          <div className="value">{credits(p.body.cash)}</div>
        </div>
        <div className="card stat">
          <div className="label">Available to trade</div>
          <div className="value">{credits(p.body.available)}</div>
        </div>
        <div className="card stat">
          <div className="label">Held by open orders</div>
          <div className="value">{credits(p.body.reserved)}</div>
        </div>
        <div className="card stat">
          <div className="label">Positions at market</div>
          <div className="value">{credits(marked)}</div>
        </div>
      </div>

      <section className="card table-scroll">
        <table>
          <caption>Positions</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Holding</th>
              <th scope="col" className="num">Chance</th>
              <th scope="col" className="num">Value at market</th>
              <th scope="col" className="num">Pays if right</th>
            </tr>
          </thead>
          <tbody>
            {p.body.positions.map((pos) => {
              const m = byId.get(pos.market);
              const v = value(pos.market, pos.position);
              return (
                <tr key={pos.market}>
                  <td>
                    <Link href={`/markets/${pos.market}`} className="row" style={{ flexWrap: "nowrap" }}>
                      <CategoryTile category={m?.category} />
                      <span style={{ fontWeight: 600 }}>{m?.title ?? `Market ${pos.market}`}</span>
                    </Link>
                  </td>
                  <td className={pos.position > 0 ? "yes" : "no"} style={{ fontWeight: 650 }}>
                    {Math.abs(pos.position).toLocaleString("en-US")} {pos.position > 0 ? "Yes" : "No"}
                  </td>
                  <td className="num">{chance(m?.chance)}</td>
                  <td className="num">{v == null ? "—" : credits(v)}</td>
                  <td className="num">{credits(Math.abs(pos.position) * 100)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {p.body.positions.length === 0 && (
          <p className="empty">
            No positions yet. <Link className="link" href="/">Find a market</Link>.
          </p>
        )}
      </section>

      <section className="card table-scroll">
        <table>
          <caption>Open orders</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Order</th>
              <th scope="col" className="num">Remaining</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {p.body.open_orders.map((o) => (
              <tr key={o.order}>
                <td>
                  <Link href={`/markets/${o.market}`}>{byId.get(o.market)?.title ?? `Market ${o.market}`}</Link>
                </td>
                <td className={o.side === "buy" ? "yes" : "no"}>{o.side === "buy" ? `Buy Yes at ${o.price}¢` : `Buy No at ${100 - o.price}¢`}</td>
                <td className="num">{o.remaining.toLocaleString("en-US")}</td>
                <td className="num">
                  <CancelButton order={o.order} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {p.body.open_orders.length === 0 && <p className="empty">No open orders.</p>}
      </section>

      <section className="card table-scroll">
        <table>
          <caption>Recent fills</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col" className="num">Yes price</th>
              <th scope="col" className="num">Contracts</th>
            </tr>
          </thead>
          <tbody>
            {(fills.ok ? fills.body.fills : []).map((f) => (
              <tr key={`${f.seq}-${f.price}-${f.qty}`}>
                <td>{byId.get(f.market)?.title ?? `Market ${f.market}`}</td>
                <td className="num">{f.price}¢</td>
                <td className="num">{f.qty.toLocaleString("en-US")}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {(!fills.ok || fills.body.fills.length === 0) && <p className="empty">No fills yet.</p>}
      </section>
    </div>
  );
}
