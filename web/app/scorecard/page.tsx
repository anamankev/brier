import Link from "next/link";
import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { score } from "@/lib/format";

export const dynamic = "force-dynamic";
export const metadata = { title: "Your scorecard" };

type Card = {
  markets: { market: number; title: string | null; outcome: string; skill: number; brier: number; market_brier: number; samples: number; forecasts: number }[];
  average_skill: number | null;
  ranked: boolean;
  min_scored_markets: number;
};

export default async function Scorecard() {
  const user = await me();
  if (!user) redirect("/signin");
  const card = await gateway<Card>("/v1/scorecard");
  if (!card.ok) return <p className="notice error">Could not load the scorecard.</p>;
  const c = card.body;
  const beat = c.markets.filter((m) => m.skill > 0).length;
  return (
    <div className="stack" style={{ gap: 24 }}>
      <div>
        <h1 className="display">Your scorecard</h1>
        <p className="dim" style={{ maxWidth: 720 }}>
          Skill measures how much better your forecasts were than the market price, using the Brier score, averaged over
          the whole time each market traded. Above zero means you beat the market. A forecast made early counts for
          longer, so being right early is worth the most.
        </p>
      </div>
      <div className="stats">
        <div className="card stat">
          <div className="label">Average skill</div>
          <div className={`value ${c.average_skill == null ? "" : c.average_skill >= 0 ? "yes" : "no"}`}>{score(c.average_skill)}</div>
        </div>
        <div className="card stat">
          <div className="label">Markets scored</div>
          <div className="value">{c.markets.length}</div>
        </div>
        <div className="card stat">
          <div className="label">Beat the market</div>
          <div className="value">{c.markets.length ? `${beat} of ${c.markets.length}` : "—"}</div>
        </div>
        <div className="card stat">
          <div className="label">Leaderboard</div>
          <div className="value" style={{ fontSize: 22, marginTop: 12 }}>
            {c.ranked ? "Ranked" : `${Math.max(0, c.min_scored_markets - c.markets.length)} more markets to rank`}
          </div>
        </div>
      </div>
      <section className="card table-scroll">
        <table>
          <caption>Scored markets</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Outcome</th>
              <th scope="col" className="num">Your Brier</th>
              <th scope="col" className="num">Market Brier</th>
              <th scope="col" className="num">Skill</th>
            </tr>
          </thead>
          <tbody>
            {c.markets.map((m) => (
              <tr key={m.market}>
                <td>
                  <Link href={`/markets/${m.market}`} style={{ fontWeight: 600 }}>
                    {m.title ?? `Market ${m.market}`}
                  </Link>
                </td>
                <td className={m.outcome === "yes" ? "yes" : "no"}>{m.outcome === "yes" ? "Yes" : "No"}</td>
                <td className="num">{score(m.brier)}</td>
                <td className="num">{score(m.market_brier)}</td>
                <td className={`num ${m.skill >= 0 ? "yes" : "no"}`} style={{ fontWeight: 650 }}>
                  {m.skill > 0 ? "+" : ""}
                  {score(m.skill)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {c.markets.length === 0 && (
          <p className="empty">
            Nothing scored yet. Record a forecast on an <Link className="link" href="/">open market</Link>; it is scored when the
            market resolves.
          </p>
        )}
      </section>
      <p className="muted" style={{ fontSize: 13 }}>
        Brier scores run from 0 (perfect) to 1. Lower is better; skill is the market&apos;s score minus yours.
      </p>
    </div>
  );
}
