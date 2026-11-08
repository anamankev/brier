import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { WS_URL, gateway, me, renderTime } from "@/lib/server";
import { cadence, categoryName, compact, label, leading, type EventSummary } from "@/lib/events";
import { getEvent } from "@/lib/events-server";
import { relative, shortDate, when } from "@/lib/format";
import type { Portfolio, ResolutionView, Rules } from "@/lib/types";
import { Repeat } from "@/components/Icons";
import { Photo } from "@/components/Photo";
import { EventBoard, type Outcome } from "@/components/EventBoard";
import { MultiChart } from "@/components/MultiChart";
import { WatchButton } from "@/components/WatchButton";

export const dynamic = "force-dynamic";

const SOURCES: Record<string, string> = {
  nws: "the National Weather Service daily climate report",
  usgs: "the USGS earthquake catalog",
  swpc: "NOAA's Space Weather Prediction Center",
  bls: "the Bureau of Labor Statistics",
  fred: "the Federal Reserve (via FRED)",
  noaa: "NOAA's GHCN-Daily record",
};
const STATUS: Record<EventSummary["status"], string> = {
  open: "Trading",
  upcoming: "Opens soon",
  closed: "Closed · waiting for the result",
  settled: "Settled",
};

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const e = await getEvent(slug);
  return { title: e?.title ?? "Market" };
}

export default async function EventPage({ params, searchParams }: { params: Promise<{ slug: string }>; searchParams: Promise<{ m?: string; side?: string }> }) {
  const [{ slug }, q] = await Promise.all([params, searchParams]);
  const e = await getEvent(slug);
  if (!e) notFound();
  const now = renderTime();
  const user = await me();
  const charted = (e.exclusive ? leading(e) : e.markets).slice(0, 4);
  const [rules, prices, portfolio, resolutions] = await Promise.all([
    Promise.all(e.markets.map((m) => gateway<Rules>(`/v1/markets/${m.market}/rules`))),
    Promise.all(charted.map((m) => gateway<{ points: [string, number][] }>(`/v1/markets/${m.market}/prices`))),
    user ? gateway<Portfolio>("/v1/portfolio") : Promise.resolve(null),
    e.status === "settled" ? Promise.all(e.markets.map((m) => gateway<ResolutionView>(`/v1/markets/${m.market}/resolution`))) : Promise.resolve([]),
  ]);
  const positions = new Map((portfolio?.ok ? portfolio.body.positions : []).map((p) => [p.market, p.position]));
  const outcomes: Outcome[] = e.markets.map((m, i) => ({
    market: m.market,
    label: label(m),
    state: m.state,
    chance: m.chance,
    change: m.change_24h,
    volume: m.volume,
    bid: m.best_bid,
    ask: m.best_ask,
    rules: rules[i]!.ok ? rules[i]!.body.text : "The rulebook could not be loaded.",
    position: positions.get(m.market) ?? 0,
  }));
  const asked = Number(q.m);
  const initial = e.markets.some((m) => m.market === asked) ? asked : leading(e)[0]!.market;
  const provider = rules.find((r) => r.ok)?.ok ? (rules.find((r) => r.ok) as { body: Rules }).body.rulebook.source.provider : "";
  const winner = e.markets.find((m, i) => {
    const r = resolutions[i];
    return r?.ok && r.body.resolutions[0]?.outcome === "yes" && r.body.resolutions[0]?.state === "settled";
  });
  const series = charted.map((m, i) => ({ name: e.markets.length > 1 ? label(m) : "Yes", points: prices[i]!.ok ? prices[i]!.body.points : [] }));
  const siblings = e.siblings ?? [];
  const at = siblings.findIndex((s) => s.slug === e.slug);
  const tabs = siblings.slice(Math.max(0, at - 3), at + 4);
  const repeat = cadence(e);

  return (
    <article className="event-page">
      <header className="event-head">
        <Photo of={e} size="lg" />
        <div className="event-title">
          <p className="kicker">
            {e.category ? <Link href={`/c/${e.category}`}>{categoryName(e.category)}</Link> : "Markets"}
            {repeat && (
              <>
                {" · "}
                <span className="inline-icon">
                  <Repeat /> {repeat}
                </span>
              </>
            )}
            {" · "}
            {STATUS[e.status]}
          </p>
          <h1 className="display">{e.title}</h1>
          <p className="meta">
            <span>{compact(e.volume)} contracts traded</span>
            <span>
              {e.status === "open" || e.status === "upcoming" ? `Trading closes ${relative(e.close, now)}` : `Trading closed ${shortDate(e.close)}`}
            </span>
            {provider && <span>Settles from {SOURCES[provider] ?? provider}</span>}
          </p>
        </div>
        <WatchButton slug={e.slug} title={e.title} />
      </header>

      {winner && (
        <p className="notice ok settled-banner">
          Settled: <strong>{label(winner)}</strong>. Payouts were made {when(e.release)} or after, once the dispute window closed.
        </p>
      )}

      {tabs.length > 1 && (
        <nav className="date-tabs" aria-label="Other days">
          {tabs.map((s) => (
            <Link key={s.slug} href={`/e/${s.slug}`} aria-current={s.slug === e.slug ? "page" : undefined}>
              {shortDate(`${s.observation}T12:00:00Z`)}
              {s.status === "settled" && <span className="dot" aria-label="settled" />}
            </Link>
          ))}
        </nav>
      )}

      <section className="card pad chart-card" aria-label="Chance over time">
        <MultiChart series={series} />
      </section>

      <EventBoard
        question={e.title}
        outcomes={outcomes}
        initial={initial}
        initialSide={q.side === "no" ? "no" : "yes"}
        signedIn={Boolean(user)}
        wsUrl={WS_URL}
      />
    </article>
  );
}
