import Link from "next/link";
import { gateway, renderTime } from "@/lib/server";
import { kind, label, type EventSummary } from "@/lib/events";
import { listEvents } from "@/lib/events-server";
import { relative } from "@/lib/format";
import { CategoryTile } from "@/components/Icons";
import { Sparkline } from "@/components/Sparkline";

export const dynamic = "force-dynamic";
export const metadata = { title: "Biggest moves" };

function moved(e: EventSummary) {
  return [...e.markets].sort((a, b) => Math.abs(b.change_24h ?? 0) - Math.abs(a.change_24h ?? 0))[0]!;
}

export default async function Movers() {
  const now = renderTime();
  const list = await listEvents({ status: "open", sort: "movers", limit: 30 });
  const rows = (list?.events ?? []).filter((e) => moved(e).change_24h);
  const points = await Promise.all(rows.map((e) => gateway<{ points: [string, number][] }>(`/v1/markets/${moved(e).market}/prices`)));
  return (
    <section className="movers-page" aria-labelledby="movers-h">
      <header className="card movers-hero">
        <p className="kicker">Last 24 hours</p>
        <h1 id="movers-h" className="display">
          Biggest moves
        </h1>
        <p className="dim">The open markets whose chance moved most since this time yesterday, from the exchange&apos;s minute prices.</p>
      </header>
      {rows.length === 0 ? (
        <p className="card empty">Nothing has moved in the last 24 hours yet.</p>
      ) : (
        <ol className="mover-list">
          {rows.map((e, i) => {
            const m = moved(e);
            const up = (m.change_24h ?? 0) > 0;
            const p = points[i]!;
            return (
              <li key={e.slug}>
                <Link href={`/e/${e.slug}?m=${m.market}`} className="mover">
                  <span className="rank">{i + 1}</span>
                  <CategoryTile category={kind(e)} />
                  <span className="what">
                    <span className="t">{e.title}</span>
                    <span className="sub">
                      {e.markets.length > 1 && <b>{label(m)} </b>}
                      <span className="big">{m.chance == null ? "—" : `${m.chance}%`}</span>
                      <span className={up ? "yes" : "no"}>
                        {up ? "▲" : "▼"} {Math.abs(m.change_24h ?? 0)} pts
                      </span>
                      <span className="muted">closes {relative(e.close, now)}</span>
                    </span>
                  </span>
                  <span className="spark">
                    <Sparkline points={p.ok ? p.body.points : []} width={150} height={44} />
                  </span>
                </Link>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
