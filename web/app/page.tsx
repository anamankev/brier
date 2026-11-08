import Link from "next/link";
import { gateway, renderTime } from "@/lib/server";
import { summaries, type Summary } from "@/lib/markets";
import { cents, chance, relative, shortDate } from "@/lib/format";
import { MarketCard } from "@/components/MarketCard";
import { CategoryTile, Chevron } from "@/components/Icons";
import { Delta } from "@/components/Delta";
import { Sparkline } from "@/components/Sparkline";

export const dynamic = "force-dynamic";

type Leaders = { leaders: { rank: number; username: string; markets: number; skill: number }[]; min_scored_markets: number };

function Featured({ m, now }: { m: Summary; now: number }) {
  return (
    <section className="card hero" aria-labelledby="featured">
      <div style={{ display: "grid", alignContent: "center" }}>
        <div className="row">
          <CategoryTile category={m.category} />
          <span className="kicker">{m.category}</span>
        </div>
        <h1 id="featured" className="display">
          <Link href={`/markets/${m.market}`}>{m.title}</Link>
        </h1>
        <div className="row" style={{ alignItems: "baseline", gap: 14 }}>
          <span className="big-number" style={{ color: "var(--yes)" }}>
            {chance(m.chance)}
          </span>
          <span className="dim">chance</span>
          <Delta change={m.change} suffix=" today" />
        </div>
        <div className="row" style={{ marginTop: 18 }}>
          <Link className="px yes" href={`/markets/${m.market}?buy=yes`}>
            Buy Yes {cents(m.yesPrice)}
          </Link>
          <Link className="px no" href={`/markets/${m.market}?buy=no`}>
            Buy No {cents(m.noPrice)}
          </Link>
        </div>
        <p className="muted" style={{ margin: "18px 0 0", fontSize: 14 }}>
          {(m.volume ?? 0).toLocaleString("en-US")} contracts traded · trading closes {relative(m.close ?? m.release, now)} · result
          published {shortDate(m.release)}
        </p>
      </div>
      <div style={{ display: "grid", alignContent: "center" }}>
        <Sparkline points={m.points} width={600} height={240} fluid />
      </div>
    </section>
  );
}

export default async function Home({ searchParams }: { searchParams: Promise<{ category?: string; q?: string }> }) {
  const { category, q } = await searchParams;
  const now = renderTime();
  const [all, leaders, health] = await Promise.all([summaries(), gateway<Leaders>("/v1/leaderboards/skill"), gateway("/v1/health")]);
  if (!health.ok) {
    return (
      <div className="card auth" role="alert">
        <h1 className="display">Markets are unavailable</h1>
        <p className="dim" style={{ margin: 0 }}>The exchange is not answering right now. Please try again in a moment.</p>
      </div>
    );
  }
  const query = q?.trim().toLowerCase();
  const filtered = all.filter((m) => (!category || m.category === category) && (!query || m.title.toLowerCase().includes(query)));
  const live = (m: Summary) => (m.state === "open" ? 0 : m.state === "pre_open" || m.state === "halted" ? 1 : 2);
  filtered.sort((a, b) => live(a) - live(b) || (b.volume ?? 0) - (a.volume ?? 0) || (a.close ?? "").localeCompare(b.close ?? ""));
  const browsing = Boolean(category || query);
  const featured = !browsing ? filtered[0] : undefined;
  const upcoming = all.filter((m) => Date.parse(m.release) > now).sort((a, b) => a.release.localeCompare(b.release));

  return (
    <>
      {featured && <Featured m={featured} now={now} />}

      {!browsing && all.length > 0 && (
        <nav className="ticker" aria-label="Live prices" style={{ marginTop: 20 }}>
          {all.map((m) => (
            <Link key={m.market} href={`/markets/${m.market}`}>
              <strong>{m.title}</strong>
              <span>{chance(m.chance)}</span>
              <Delta change={m.change} />
            </Link>
          ))}
        </nav>
      )}

      <div className="with-rail" style={{ marginTop: 28 }}>
        <div>
          <h2 className="section">
            {query ? `Results for “${q}”` : category ? category[0]!.toUpperCase() + category.slice(1) : "All markets"}
          </h2>
          {filtered.length === 0 ? (
            <p className="card empty">No markets match.</p>
          ) : (
            <div className="grid-2">
              {filtered.map((m) => (
                <MarketCard key={m.market} m={m} />
              ))}
            </div>
          )}
        </div>

        <aside className="stack" aria-label="More">
          <section className="card" aria-labelledby="upcoming-h">
            <h2 id="upcoming-h" className="section" style={{ padding: "18px 18px 0", fontSize: 26 }}>
              Coming up
            </h2>
            <div className="list">
              {upcoming.slice(0, 5).map((m) => {
                const d = new Date(m.release);
                return (
                  <Link key={m.market} href={`/markets/${m.market}`} className="row" style={{ flexWrap: "nowrap" }}>
                    <span className="date-tile">
                      <span>{d.toLocaleDateString("en-US", { month: "short", timeZone: "UTC" }).toUpperCase()}</span>
                      <b>{d.getUTCDate()}</b>
                    </span>
                    <span style={{ minWidth: 0 }}>
                      <span style={{ display: "block", fontWeight: 600, fontSize: 14 }}>{m.title}</span>
                      <span className="muted" style={{ fontSize: 13 }}>
                        Result published · {chance(m.chance)} chance now
                      </span>
                    </span>
                  </Link>
                );
              })}
              {upcoming.length === 0 && <p className="empty">No releases scheduled.</p>}
            </div>
          </section>

          <section className="card" aria-labelledby="leaders-h">
            <div className="spread" style={{ padding: "18px 18px 0" }}>
              <h2 id="leaders-h" className="section" style={{ fontSize: 26, margin: 0 }}>
                Top forecasters
              </h2>
              <Link href="/leaderboards" className="muted" aria-label="All leaderboards">
                <Chevron />
              </Link>
            </div>
            <div className="list" style={{ marginTop: 12 }}>
              {leaders.ok && leaders.body.leaders.length > 0 ? (
                leaders.body.leaders.slice(0, 5).map((l) => (
                  <div key={l.username} className="spread">
                    <span>
                      <span className="muted" style={{ marginRight: 10 }}>{l.rank}</span>
                      {l.username}
                    </span>
                    <span className="yes" style={{ fontWeight: 650 }}>+{(l.skill / 100).toFixed(1)}</span>
                  </div>
                ))
              ) : (
                <p className="muted" style={{ fontSize: 14, margin: 0 }}>
                  Forecasters appear here after {leaders.ok ? leaders.body.min_scored_markets : 10} scored markets. Record a
                  probability on any open market to start.
                </p>
              )}
            </div>
          </section>

          <section className="card pad" aria-labelledby="how-h">
            <h2 id="how-h" className="section" style={{ fontSize: 26 }}>
              How every market settles
            </h2>
            <ol className="steps">
              <li>
                <span className="step-n">1</span>
                <span>
                  <b>Rules fixed at listing</b>A fingerprint of the rulebook goes into the exchange&apos;s log, so the rules
                  cannot change once trading starts.
                </span>
              </li>
              <li>
                <span className="step-n">2</span>
                <span>
                  <b>Settled from the public source</b>BLS, FRED, or NOAA. The response is kept, and anyone can check the
                  outcome against it.
                </span>
              </li>
              <li>
                <span className="step-n">3</span>
                <span>
                  <b>Two admins and 24 hours</b>A second admin confirms, then holders have 24 hours to dispute before
                  payout.
                </span>
              </li>
            </ol>
          </section>
        </aside>
      </div>
    </>
  );
}
