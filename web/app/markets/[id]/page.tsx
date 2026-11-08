import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { WS_URL, gateway, me, renderTime } from "@/lib/server";
import { summaries } from "@/lib/markets";
import { chance, relative, when } from "@/lib/format";
import type { Portfolio, ResolutionView, Rules } from "@/lib/types";
import { MarketLive } from "@/components/MarketLive";
import { ForecastPanel } from "@/components/ForecastPanel";
import { DisputeForm } from "@/components/DisputeForm";
import { CategoryTile, Check } from "@/components/Icons";
import { Delta } from "@/components/Delta";

export const dynamic = "force-dynamic";

const OUTCOME = { yes: "Yes", no: "No", void: "Void" } as const;
const STATE_WORDS: Record<string, string> = {
  pre_open: "Opens soon",
  open: "Trading",
  halted: "Halted",
  closed: "Closed · awaiting result",
  resolved: "Resolved",
  voided: "Voided",
};

export async function generateMetadata({ params }: { params: Promise<{ id: string }> }): Promise<Metadata> {
  const { id } = await params;
  const rules = await gateway<Rules>(`/v1/markets/${Number(id)}/rules`);
  return { title: rules.ok ? rules.body.rulebook.title : "Market" };
}

export default async function MarketPage({ params, searchParams }: { params: Promise<{ id: string }>; searchParams: Promise<{ buy?: string }> }) {
  const [{ id }, { buy }] = await Promise.all([params, searchParams]);
  const market = Number(id);
  if (!Number.isInteger(market) || market <= 0) notFound();
  const [all, rules, resolution, trades, user] = await Promise.all([
    summaries(),
    gateway<Rules>(`/v1/markets/${market}/rules`),
    gateway<ResolutionView>(`/v1/markets/${market}/resolution`),
    gateway<{ trades: { seq: number; price: number; qty: number; taker_side: string }[] }>(`/v1/markets/${market}/trades?limit=20`),
    me(),
  ]);
  const m = all.find((s) => s.market === market);
  if (!m || !rules.ok) notFound();
  const r = rules.body;
  const now = renderTime();
  const [portfolio, forecasts] = user
    ? await Promise.all([gateway<Portfolio>("/v1/portfolio"), gateway<{ forecasts: { market: number; probability: number; at: string }[] }>("/v1/forecasts")])
    : [null, null];
  const position = portfolio?.ok ? (portfolio.body.positions.find((p) => p.market === market)?.position ?? 0) : 0;
  const myForecast = forecasts?.ok ? (forecasts.body.forecasts.find((f) => f.market === market) ?? null) : null;
  const open = m.state === "open" || m.state === "halted";
  const res = resolution.ok ? resolution.body : null;
  const latest = res?.resolutions[0];
  const canDispute = Boolean(user && position !== 0 && latest?.state === "confirmed" && latest.dispute_until && Date.parse(latest.dispute_until) > now);
  const settled = latest?.state === "settled";
  const steps: { label: string; at: string | null; done: boolean }[] = [
    { label: "Trading opens", at: r.schedule.open, done: Date.parse(r.schedule.open) <= now },
    { label: "Trading closes", at: r.schedule.close, done: Date.parse(r.schedule.close) <= now },
    { label: `Value published by ${r.rulebook.source.provider.toUpperCase()}`, at: r.schedule.release, done: Date.parse(r.schedule.release) <= now },
    { label: "Outcome proposed and confirmed", at: latest?.confirmed_at ?? null, done: Boolean(latest?.confirmed_at) },
    { label: "Dispute window ends, payout", at: latest?.dispute_until ?? null, done: settled },
  ];

  const header = (
    <header className="stack" style={{ gap: 14 }}>
      <div className="mhead">
        <CategoryTile category={m.category} large />
        <div>
          <div className="kicker">
            {m.category} · {STATE_WORDS[m.state]}
          </div>
          <h1 className="display" style={{ marginTop: 6 }}>
            {r.rulebook.title}
          </h1>
        </div>
      </div>
      <p className="dim" style={{ margin: 0 }}>
        {r.rulebook.question}
      </p>
      <div className="row" style={{ alignItems: "baseline", gap: 14 }}>
        <span className="big-number" style={{ color: m.chance == null ? "var(--muted)" : "var(--yes)" }}>
          {chance(m.chance)}
        </span>
        <span className="dim">chance</span>
        <Delta change={m.change} suffix=" today" />
      </div>
      <p className="muted" style={{ fontSize: 14, margin: 0 }}>
        {(m.volume ?? 0).toLocaleString("en-US")} contracts traded ·{" "}
        {Date.parse(r.schedule.close) > now ? `trading closes ${relative(r.schedule.close, now)}` : `trading closed ${when(r.schedule.close)}`}
      </p>
    </header>
  );

  const below = (
    <>
      <section className="card pad" aria-labelledby="rules-h">
        <div className="spread" style={{ marginBottom: 12 }}>
          <h2 id="rules-h" className="section" style={{ margin: 0, fontSize: 26 }}>
            Market rules
          </h2>
          {r.journal?.matches ? (
            <span className="chip ok">
              <Check /> Pinned in the exchange log
            </span>
          ) : (
            <span className="chip warn">Not yet checked against the log</span>
          )}
        </div>
        <p style={{ margin: "0 0 16px", lineHeight: 1.6 }}>{r.text}</p>
        <p className="muted" style={{ fontSize: 14, margin: "0 0 16px" }}>
          Source:{" "}
          <a className="link" href={r.rulebook.source.url}>
            {r.rulebook.source.provider.toUpperCase()} series {r.rulebook.source.series}
          </a>
          . The rulebook&apos;s SHA-256 is <code>{r.rules_hash.slice(0, 16)}…</code>
          {r.journal && <> , recorded at log entry {r.journal.listed_seq.toLocaleString("en-US")} when the market was listed</>}; changing a
          single character of the rules would change it.
        </p>
        <h3 style={{ marginBottom: 6 }}>Timeline and payout</h3>
        <ol className="timeline">
          {steps.map((s, i) => {
            const isNext = !s.done && (i === 0 || steps[i - 1]!.done);
            return (
              <li key={s.label} className={s.done ? "done" : isNext ? "now" : ""}>
                <span className="dot" aria-hidden="true" />
                <span>
                  {s.label}
                  <span className="sr-only">{s.done ? " (done)" : isNext ? " (next)" : ""}</span>
                </span>
                <span className="muted">{s.at ? when(s.at) : "—"}</span>
              </li>
            );
          })}
        </ol>
        <details style={{ marginTop: 14 }}>
          <summary>Full rulebook (canonical JSON)</summary>
          <pre>{r.canonical}</pre>
        </details>
      </section>

      <section className="card pad" aria-labelledby="res-h">
        <h2 id="res-h" className="section" style={{ fontSize: 26 }}>
          Resolution and evidence
        </h2>
        {!res || res.resolutions.length === 0 ? (
          <p className="muted" style={{ margin: 0 }}>
            No outcome yet. After {when(r.schedule.release)} the resolver fetches the value from the source, stores the
            response, and proposes an outcome; a second admin must confirm it, and holders then have 24 hours to dispute.
          </p>
        ) : (
          <div className="stack">
            {res.resolutions.map((x) => (
              <article key={x.resolution} className="card pad" style={{ background: "var(--bg-2)" }}>
                <div className="spread">
                  <div className="row">
                    <strong style={{ fontSize: 18 }} className={x.outcome === "yes" ? "yes" : x.outcome === "no" ? "no" : ""}>
                      {OUTCOME[x.outcome]}
                    </strong>
                    {x.void_condition && <span className="muted">({x.void_condition.replaceAll("_", " ")})</span>}
                  </div>
                  <span className={`chip ${x.state === "settled" ? "ok" : x.state === "withdrawn" ? "bad" : "warn"}`}>{x.state}</span>
                </div>
                <p className="muted" style={{ fontSize: 14, margin: "8px 0" }}>
                  Proposed {when(x.proposed_at)}
                  {x.confirmed_at && ` · confirmed ${when(x.confirmed_at)}`}
                  {x.state === "confirmed" && x.dispute_until && ` · disputes open until ${when(x.dispute_until)}`}
                  {x.disputes.total > 0 && ` · ${x.disputes.total} dispute${x.disputes.total === 1 ? "" : "s"}`}
                  {x.settled_seq !== null && ` · paid out at log entry ${x.settled_seq.toLocaleString("en-US")}`}
                </p>
                {typeof x.evidence.value === "string" && (
                  <p style={{ margin: "0 0 8px" }}>
                    Published value: <strong>{x.evidence.value}</strong> (the rule: {r.rulebook.condition.op}{" "}
                    {r.rulebook.condition.threshold})
                  </p>
                )}
                <details>
                  <summary>
                    Evidence record{" "}
                    {res.journal?.evidence_hash === x.evidence_hash && (
                      <span className="chip ok" style={{ marginLeft: 6 }}>
                        <Check /> Matches the exchange log
                      </span>
                    )}
                  </summary>
                  <p className="muted" style={{ fontSize: 13 }}>
                    SHA-256 <code>{x.evidence_hash}</code>
                  </p>
                  <pre>{JSON.stringify(x.evidence, null, 2)}</pre>
                  {r.rulebook.source.provider === "bls" && (
                    <p className="muted" style={{ fontSize: 12.5 }}>
                      Source: U.S. Bureau of Labor Statistics. BLS.gov cannot vouch for the data or analyses derived from
                      these data after the data have been retrieved from BLS.gov.
                    </p>
                  )}
                </details>
              </article>
            ))}
          </div>
        )}
        {canDispute && latest && <DisputeForm resolution={latest.resolution} />}
      </section>
    </>
  );

  const rail =
    user && user.role !== "resolver" ? (
      <ForecastPanel market={market} open={open} marketChance={m.chance} initial={myForecast} />
    ) : null;

  return (
    <MarketLive
      market={market}
      points={m.points}
      recent={trades.ok ? trades.body.trades : []}
      wsUrl={WS_URL}
      signedIn={Boolean(user)}
      canTrade={m.state === "open" && user?.role !== "resolver"}
      position={position}
      initialSide={buy === "no" ? "no" : "yes"}
      initialQuotes={{
        buyYes: m.yesPrice,
        buyNo: m.noPrice,
        sellYes: m.noPrice == null ? null : 100 - m.noPrice,
        sellNo: m.yesPrice == null ? null : 100 - m.yesPrice,
      }}
      header={header}
      below={below}
      rail={rail}
    />
  );
}
