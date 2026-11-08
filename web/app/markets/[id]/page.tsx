import { notFound } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { STATE_LABEL, when } from "@/lib/format";
import type { MarketRow, Portfolio, ResolutionView, Rules } from "@/lib/types";
import { MarketLive } from "@/components/MarketLive";
import { TradeTicket } from "@/components/TradeTicket";
import { ForecastPanel } from "@/components/ForecastPanel";
import { DisputeForm } from "@/components/DisputeForm";

export const dynamic = "force-dynamic";

const OUTCOME = { yes: "YES", no: "NO", void: "Void (each side gets 50)" } as const;

export default async function MarketPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const market = Number(id);
  if (!Number.isInteger(market) || market <= 0) notFound();
  const [list, rules, resolution, prices, trades, user] = await Promise.all([
    gateway<{ markets: MarketRow[] }>("/v1/markets"),
    gateway<Rules>(`/v1/markets/${market}/rules`),
    gateway<ResolutionView>(`/v1/markets/${market}/resolution`),
    gateway<{ points: [string, number][] }>(`/v1/markets/${market}/prices`),
    gateway<{ trades: { seq: number; price: number; qty: number; taker_side: string }[] }>(`/v1/markets/${market}/trades?limit=20`),
    me(),
  ]);
  const row = list.ok ? list.body.markets.find((m) => m.market === market) : undefined;
  if (!row || !rules.ok) notFound();
  const r = rules.body;
  const portfolio = user ? await gateway<Portfolio>("/v1/portfolio") : null;
  const position = portfolio?.ok ? (portfolio.body.positions.find((p) => p.market === market)?.position ?? 0) : 0;
  const open = row.state === "open" || row.state === "halted";
  const latest = resolution.ok ? resolution.body.resolutions[0] : undefined;
  const canDispute =
    user && position !== 0 && latest?.state === "confirmed" && latest.dispute_until && Date.parse(latest.dispute_until) > Date.now();

  return (
    <>
      <p className="muted" style={{ margin: 0 }}>
        {row.category} · <span className={`badge ${row.state}`}>{STATE_LABEL[row.state]}</span> · trading closes {when(r.schedule.close)}
      </p>
      <h1>{r.rulebook.title}</h1>
      <p>{r.rulebook.question}</p>

      <MarketLive market={market} points={prices.ok ? prices.body.points : []} recent={trades.ok ? trades.body.trades : []} />

      <div className="grid market-grid">
        <section className="card" aria-labelledby="trade-h">
          <h2 id="trade-h">Trade</h2>
          {!user ? (
            <p>
              <a href="/signin">Sign in</a> to trade. New accounts start with 10,000 play credits.
            </p>
          ) : row.state !== "open" ? (
            <p className="muted">This market is not open for trading.</p>
          ) : user.role === "resolver" ? (
            <p className="muted">Resolver accounts cannot trade.</p>
          ) : (
            <TradeTicket market={market} />
          )}
          {user && position !== 0 && (
            <p>
              You hold <strong>{Math.abs(position)}</strong> {position > 0 ? "YES" : "NO"} contracts.
            </p>
          )}
        </section>
        <section className="card" aria-labelledby="forecast-h">
          <h2 id="forecast-h">Forecast</h2>
          {user && user.role !== "resolver" ? (
            <ForecastPanel market={market} open={open} />
          ) : (
            <p className="muted">Sign in to record your own probability and be scored against the market.</p>
          )}
        </section>
      </div>

      <section className="card" aria-labelledby="rules-h">
        <h2 id="rules-h">Rules</h2>
        <p>{r.text}</p>
        <table>
          <tbody>
            <tr>
              <th scope="row">Source</th>
              <td>
                <a href={r.rulebook.source.url}>
                  {r.rulebook.source.provider.toUpperCase()} {r.rulebook.source.series}
                </a>
              </td>
            </tr>
            <tr>
              <th scope="row">Trading</th>
              <td>
                {when(r.schedule.open)} to {when(r.schedule.close)}
              </td>
            </tr>
            <tr>
              <th scope="row">Value published</th>
              <td>{when(r.schedule.release)}</td>
            </tr>
            <tr>
              <th scope="row">Resolution deadline</th>
              <td>{when(r.schedule.deadline)}</td>
            </tr>
            <tr>
              <th scope="row">Rules hash</th>
              <td>
                <code>{r.rules_hash}</code>{" "}
                {r.journal ? (
                  r.journal.matches ? (
                    <span className="badge open">Matches the journal (seq {r.journal.listed_seq})</span>
                  ) : (
                    <span className="badge" style={{ color: "var(--no)" }}>
                      Does not match the journal
                    </span>
                  )
                ) : (
                  <span className="muted">journal not yet checked</span>
                )}
              </td>
            </tr>
          </tbody>
        </table>
        <details style={{ marginTop: 10 }}>
          <summary>The rulebook (canonical JSON; its SHA-256 is the rules hash)</summary>
          <pre>{r.canonical}</pre>
        </details>
      </section>

      <section className="card" aria-labelledby="res-h">
        <h2 id="res-h">Resolution</h2>
        {!resolution.ok || resolution.body.resolutions.length === 0 ? (
          <p className="muted">No outcome proposed yet. A resolver fetches the source after {when(r.schedule.release)}.</p>
        ) : (
          <ol className="timeline">
            {resolution.body.resolutions.map((res) => (
              <li key={res.resolution} className={res.state}>
                <p style={{ margin: 0 }}>
                  <strong>{OUTCOME[res.outcome]}</strong> — {res.state}
                  {res.void_condition && ` (${res.void_condition.replaceAll("_", " ")})`}
                </p>
                <p className="muted" style={{ margin: 0 }}>
                  Proposed {when(res.proposed_at)}
                  {res.confirmed_at && ` · confirmed ${when(res.confirmed_at)}`}
                  {res.dispute_until && res.state === "confirmed" && ` · open to disputes until ${when(res.dispute_until)}`}
                  {res.settled_seq !== null && ` · settled at journal seq ${res.settled_seq}`}
                  {res.disputes.total > 0 && ` · ${res.disputes.total} dispute(s), ${res.disputes.open} open`}
                </p>
                <details>
                  <summary>Evidence</summary>
                  <p>
                    Evidence hash <code>{res.evidence_hash}</code>
                    {resolution.body.journal?.evidence_hash === res.evidence_hash && (
                      <span className="badge open" style={{ marginLeft: 6 }}>
                        Matches the journal
                      </span>
                    )}
                  </p>
                  <pre>{JSON.stringify(res.evidence, null, 2)}</pre>
                  {r.rulebook.source.provider === "bls" && (
                    <p className="muted" style={{ fontSize: "0.85rem" }}>
                      Source: U.S. Bureau of Labor Statistics. BLS.gov cannot vouch for the data or analyses derived from
                      these data after the data have been retrieved from BLS.gov.
                    </p>
                  )}
                </details>
              </li>
            ))}
          </ol>
        )}
        {canDispute && latest && <DisputeForm resolution={latest.resolution} />}
      </section>
    </>
  );
}
