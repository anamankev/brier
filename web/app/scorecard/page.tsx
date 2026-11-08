import Link from "next/link";
import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { score } from "@/lib/format";

export const dynamic = "force-dynamic";

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
  return (
    <>
      <h1>Your forecasting scorecard</h1>
      <p className="muted">
        Skill is how much better your forecasts were than the market price, by the Brier score, averaged over the
        whole time each market was open. Above 0 means you beat the market. Forecasting early counts for more.
      </p>
      <section className="card">
        <p>
          Average skill: <strong>{score(c.average_skill)}</strong>{" "}
          {c.ranked ? "(on the leaderboard)" : `(${c.markets.length} of ${c.min_scored_markets} scored markets needed for the leaderboard)`}
        </p>
        <table>
          <caption>Scored markets</caption>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Outcome</th>
              <th scope="col" className="num">
                Your Brier
              </th>
              <th scope="col" className="num">
                Market Brier
              </th>
              <th scope="col" className="num">
                Skill
              </th>
            </tr>
          </thead>
          <tbody>
            {c.markets.map((m) => (
              <tr key={m.market}>
                <td>
                  <Link href={`/markets/${m.market}`}>{m.title ?? `Market ${m.market}`}</Link>
                </td>
                <td>{m.outcome.toUpperCase()}</td>
                <td className="num">{score(m.brier)}</td>
                <td className="num">{score(m.market_brier)}</td>
                <td className={`num ${m.skill >= 0 ? "yes" : "no"}`}>{score(m.skill)}</td>
              </tr>
            ))}
            {c.markets.length === 0 && (
              <tr>
                <td colSpan={5} className="muted">
                  Nothing scored yet: record a forecast on an open market, and it is scored when the market resolves.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
    </>
  );
}
