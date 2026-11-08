import Link from "next/link";
import { gateway } from "@/lib/server";
import { credits, score } from "@/lib/format";

export const dynamic = "force-dynamic";
export const metadata = { title: "Leaderboards" };

type Skill = { min_scored_markets: number; leaders: { rank: number; username: string; markets: number; skill: number }[] };
type Profit = { leaders: { rank: number; username: string; markets: number; profit: number }[] };

export default async function Leaderboards() {
  const [skill, profit] = await Promise.all([gateway<Skill>("/v1/leaderboards/skill"), gateway<Profit>("/v1/leaderboards/profit")]);
  const min = skill.ok ? skill.body.min_scored_markets : 10;
  return (
    <div className="stack" style={{ gap: 24 }}>
      <div>
        <h1 className="display">Leaderboards</h1>
        <p className="dim" style={{ maxWidth: 720 }}>
          Two ways to be good at this: forecasting better than the market, and trading profitably. Credits are play money,
          so the forecasting record is the one to be proud of.
        </p>
      </div>
      <div className="grid-2" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(min(100%, 420px), 1fr))" }}>
        <section className="card table-scroll">
          <table>
            <caption>
              Forecasting skill <span className="muted" style={{ fontWeight: 500, fontSize: 13 }}>· at least {min} scored markets</span>
            </caption>
            <thead>
              <tr>
                <th scope="col">#</th>
                <th scope="col">Forecaster</th>
                <th scope="col" className="num">Markets</th>
                <th scope="col" className="num">Skill</th>
              </tr>
            </thead>
            <tbody>
              {(skill.ok ? skill.body.leaders : []).map((l) => (
                <tr key={l.username}>
                  <td className="muted">{l.rank}</td>
                  <td style={{ fontWeight: 600 }}>
                    <Link className="link-quiet" href={`/u/${encodeURIComponent(l.username)}`}>{l.username}</Link>
                  </td>
                  <td className="num">{l.markets}</td>
                  <td className={`num ${l.skill >= 0 ? "yes" : "no"}`} style={{ fontWeight: 650 }}>{score(l.skill)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {(!skill.ok || skill.body.leaders.length === 0) && <p className="empty">No one has {min} scored markets yet.</p>}
        </section>
        <section className="card table-scroll">
          <table>
            <caption>Settled trading profit</caption>
            <thead>
              <tr>
                <th scope="col">#</th>
                <th scope="col">Trader</th>
                <th scope="col" className="num">Markets</th>
                <th scope="col" className="num">Profit</th>
              </tr>
            </thead>
            <tbody>
              {(profit.ok ? profit.body.leaders : []).map((l) => (
                <tr key={l.username}>
                  <td className="muted">{l.rank}</td>
                  <td style={{ fontWeight: 600 }}>
                    <Link className="link-quiet" href={`/u/${encodeURIComponent(l.username)}`}>{l.username}</Link>
                  </td>
                  <td className="num">{l.markets}</td>
                  <td className={`num ${l.profit >= 0 ? "yes" : "no"}`} style={{ fontWeight: 650 }}>
                    {l.profit > 0 ? "+" : ""}
                    {credits(l.profit)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {(!profit.ok || profit.body.leaders.length === 0) && <p className="empty">No settled markets yet.</p>}
        </section>
      </div>
    </div>
  );
}
