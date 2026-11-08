import { gateway } from "@/lib/server";
import { credits, score } from "@/lib/format";

export const dynamic = "force-dynamic";

type Skill = { min_scored_markets: number; leaders: { rank: number; username: string; markets: number; skill: number }[] };
type Profit = { leaders: { rank: number; username: string; markets: number; profit: number }[] };

export default async function Leaderboards() {
  const [skill, profit] = await Promise.all([gateway<Skill>("/v1/leaderboards/skill"), gateway<Profit>("/v1/leaderboards/profit")]);
  return (
    <>
      <h1>Leaderboards</h1>
      <div className="grid market-grid">
        <section className="card">
          <table>
            <caption>Forecasting skill (at least {skill.ok ? skill.body.min_scored_markets : 10} scored markets)</caption>
            <thead>
              <tr>
                <th scope="col">Rank</th>
                <th scope="col">Forecaster</th>
                <th scope="col" className="num">
                  Markets
                </th>
                <th scope="col" className="num">
                  Skill
                </th>
              </tr>
            </thead>
            <tbody>
              {(skill.ok ? skill.body.leaders : []).map((l) => (
                <tr key={l.username}>
                  <td>{l.rank}</td>
                  <td>{l.username}</td>
                  <td className="num">{l.markets}</td>
                  <td className="num">{score(l.skill)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section className="card">
          <table>
            <caption>Settled trading profit (credits)</caption>
            <thead>
              <tr>
                <th scope="col">Rank</th>
                <th scope="col">Trader</th>
                <th scope="col" className="num">
                  Markets
                </th>
                <th scope="col" className="num">
                  Profit
                </th>
              </tr>
            </thead>
            <tbody>
              {(profit.ok ? profit.body.leaders : []).map((l) => (
                <tr key={l.username}>
                  <td>{l.rank}</td>
                  <td>{l.username}</td>
                  <td className="num">{l.markets}</td>
                  <td className={`num ${l.profit >= 0 ? "yes" : "no"}`}>{credits(l.profit)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>
    </>
  );
}
