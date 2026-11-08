import Link from "next/link";
import { gateway, renderTime } from "@/lib/server";
import { CATEGORIES, cadence, categoryName, kind, label, leading, noPrice, yesPrice, type EventSummary } from "@/lib/events";
import { listEvents } from "@/lib/events-server";
import { cents, relative } from "@/lib/format";
import { EventCard } from "@/components/EventCard";
import { CategoryTile, Chevron } from "@/components/Icons";
import { MultiChart, type Series } from "@/components/MultiChart";

export const dynamic = "force-dynamic";

async function history(e: EventSummary, n: number): Promise<Series[]> {
  const top = leading(e).slice(0, n);
  const answers = await Promise.all(top.map((m) => gateway<{ points: [string, number][] }>(`/v1/markets/${m.market}/prices`)));
  return top.map((m, i) => ({ name: label(m), points: answers[i]!.ok ? answers[i]!.body.points : [] }));
}

function Move({ e, now }: { e: EventSummary; now: number }) {
  const m = [...e.markets].sort((a, b) => Math.abs(b.change_24h ?? 0) - Math.abs(a.change_24h ?? 0))[0]!;
  const up = (m.change_24h ?? 0) >= 0;
  return (
    <Link href={`/e/${e.slug}?m=${m.market}`} className="move">
      <span className="what">
        <span className="t">{e.title}</span>
        <span className="muted">
          {e.markets.length > 1 ? `${label(m)} · ` : ""}closes {relative(e.close, now)}
        </span>
      </span>
      <span className="num">
        <b>{m.chance == null ? "—" : `${m.chance}%`}</b>
        {m.change_24h ? (
          <span className={up ? "yes" : "no"}>
            {up ? "▲" : "▼"} {Math.abs(m.change_24h)}
          </span>
        ) : null}
      </span>
    </Link>
  );
}

export default async function Home() {
  const now = renderTime();
  const [busiest, movers, closing, health] = await Promise.all([
    listEvents({ status: "open", sort: "volume_24h", limit: 60 }),
    listEvents({ status: "open", sort: "movers", limit: 6 }),
    listEvents({ status: "open", sort: "closing", limit: 6 }),
    gateway("/v1/health"),
  ]);
  if (!health.ok || !busiest) {
    return (
      <div className="card auth" role="alert">
        <h1 className="display">Markets are unavailable</h1>
        <p className="dim" style={{ margin: 0 }}>The exchange is not answering right now. Please try again in a moment.</p>
      </div>
    );
  }
  const events = busiest.events;
  const featured = events.find((e) => e.markets.length > 2) ?? events[0];
  const series = featured ? await history(featured, 4) : [];
  const moving = (movers?.events ?? []).filter((e) => e.markets.some((m) => m.change_24h));

  return (
    <>
      {featured && (
        <div className="home-top">
          <section className="card feature" aria-labelledby="featured">
            <div className="feature-head">
              <CategoryTile category={kind(featured)} large />
              <div>
                <p className="kicker">
                  {categoryName(featured.category)}
                  {cadence(featured) ? ` · ${cadence(featured)}` : ""} · closes {relative(featured.close, now)}
                </p>
                <h1 id="featured" className="display">
                  <Link href={`/e/${featured.slug}`}>{featured.title}</Link>
                </h1>
              </div>
            </div>
            <div className="feature-body">
              <ul className="feature-outcomes">
                {leading(featured)
                  .slice(0, 4)
                  .map((m) => (
                    <li key={m.market}>
                      <Link href={`/e/${featured.slug}?m=${m.market}`} className="name">
                        {label(m)}
                      </Link>
                      <b>{m.chance == null ? "—" : `${m.chance}%`}</b>
                      <span className="buy">
                        <Link className="px yes" href={`/e/${featured.slug}?m=${m.market}&side=yes`}>
                          Yes {cents(yesPrice(m))}
                        </Link>
                        <Link className="px no" href={`/e/${featured.slug}?m=${m.market}&side=no`}>
                          No {cents(noPrice(m))}
                        </Link>
                      </span>
                    </li>
                  ))}
              </ul>
              <MultiChart series={series} height={240} />
            </div>
          </section>

          <aside className="stack" aria-label="What is moving">
            <section className="card rail" aria-labelledby="movers-h">
              <div className="spread">
                <h2 id="movers-h">Biggest moves today</h2>
                <Link href="/movers" className="muted" aria-label="All movers">
                  <Chevron />
                </Link>
              </div>
              {moving.length ? (
                moving.slice(0, 5).map((e) => <Move key={e.slug} e={e} now={now} />)
              ) : (
                <p className="muted empty-line">No moves in the last 24 hours yet.</p>
              )}
            </section>
            <section className="card rail" aria-labelledby="closing-h">
              <h2 id="closing-h">Closing soon</h2>
              {(closing?.events ?? []).slice(0, 4).map((e) => {
                const top = leading(e)[0];
                return (
                  <Link key={e.slug} href={`/e/${e.slug}`} className="move">
                    <span className="what">
                      <span className="t">{e.title}</span>
                      <span className="muted">closes {relative(e.close, now)}</span>
                    </span>
                    <span className="num">
                      <b>{top?.chance == null ? "—" : `${top.chance}%`}</b>
                      <span className="muted">{top && e.markets.length > 1 ? label(top) : "Yes"}</span>
                    </span>
                  </Link>
                );
              })}
            </section>
          </aside>
        </div>
      )}

      <section aria-labelledby="all-h" style={{ marginTop: 32 }}>
        <div className="spread wrap-gap">
          <h2 id="all-h" className="section" style={{ margin: 0 }}>
            All markets
          </h2>
          <nav className="chips" aria-label="Topics">
            {CATEGORIES.filter((c) => (busiest.categories[c.slug] ?? 0) > 0).map((c) => (
              <Link key={c.slug} href={`/c/${c.slug}`} className="chip-link">
                {c.name} <span>{busiest.categories[c.slug]}</span>
              </Link>
            ))}
          </nav>
        </div>
        {events.length === 0 ? (
          <p className="card empty" style={{ marginTop: 16 }}>
            No markets are open right now. The catalogue lists the next two days of every series as soon as forecasts
            are out.
          </p>
        ) : (
          <div className="grid-3" style={{ marginTop: 16 }}>
            {events.map((e) => (
              <EventCard key={e.slug} e={e} now={now} />
            ))}
          </div>
        )}
      </section>

      <section className="card pad settle-strip" aria-labelledby="how-h">
        <h2 id="how-h">How every market settles</h2>
        <ol>
          <li>
            <b>Rules fixed at listing.</b> A fingerprint of each market&apos;s rulebook is written into the exchange&apos;s
            log, so the rules cannot change once trading starts.
          </li>
          <li>
            <b>Settled from the public record.</b> The National Weather Service, USGS, NOAA, the BLS, or the Fed. The
            response is kept, and anyone can recheck the outcome against it.
          </li>
          <li>
            <b>Checked twice before payout.</b> A second fetch must agree, or a second admin must confirm. Then holders
            have a window to dispute.
          </li>
        </ol>
      </section>
    </>
  );
}
