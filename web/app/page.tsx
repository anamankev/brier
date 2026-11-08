import Link from "next/link";
import { gateway } from "@/lib/server";
import { STATE_LABEL, percent, when } from "@/lib/format";
import type { MarketRow } from "@/lib/types";

export const dynamic = "force-dynamic";

const CATEGORIES = ["economics", "rates", "weather"] as const;

export default async function Markets({ searchParams }: { searchParams: Promise<{ category?: string }> }) {
  const { category } = await searchParams;
  const answer = await gateway<{ markets: MarketRow[] }>("/v1/markets");
  if (!answer.ok) {
    return <p className="notice error">The exchange is not answering right now.</p>;
  }
  const listed = answer.body.markets.filter((m) => m.title);
  const shown = category ? listed.filter((m) => m.category === category) : listed;
  // Open markets first, then by close time.
  const order = (m: MarketRow) => (m.state === "open" ? 0 : m.state === "pre_open" ? 1 : 2);
  shown.sort((a, b) => order(a) - order(b) || (a.close ?? "").localeCompare(b.close ?? ""));
  return (
    <>
      <h1>Markets</h1>
      <p className="muted">
        Each price is the market&apos;s probability that the event happens: a YES contract pays 100 credits if it does.
        Every market resolves from a public source, under rules fixed when it was listed.
      </p>
      <nav aria-label="Categories" className="row" style={{ margin: "14px 0" }}>
        <Link href="/" aria-current={!category ? "page" : undefined}>
          All
        </Link>
        {CATEGORIES.map((c) => (
          <Link key={c} href={`/?category=${c}`} aria-current={category === c ? "page" : undefined}>
            {c[0]!.toUpperCase() + c.slice(1)}
          </Link>
        ))}
      </nav>
      <section className="card">
        <table>
          <caption>{shown.length === 1 ? "1 market" : `${shown.length} markets`}</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Category</th>
              <th scope="col">State</th>
              <th scope="col" className="num">
                Chance (last trade)
              </th>
              <th scope="col" className="num">
                Volume
              </th>
              <th scope="col">Trading closes</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((m) => (
              <tr key={m.market}>
                <td>
                  <Link href={`/markets/${m.market}`}>{m.title}</Link>
                </td>
                <td>{m.category}</td>
                <td>
                  <span className={`badge ${m.state}`}>{STATE_LABEL[m.state]}</span>
                </td>
                <td className="num">{percent(m.last_price)}</td>
                <td className="num">{(m.volume ?? 0).toLocaleString("en-US")}</td>
                <td>{when(m.close)}</td>
              </tr>
            ))}
            {shown.length === 0 && (
              <tr>
                <td colSpan={6} className="muted">
                  No markets here yet.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
    </>
  );
}
