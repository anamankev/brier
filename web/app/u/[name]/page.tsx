import Link from "next/link";
import { notFound } from "next/navigation";
import { gateway, renderTime } from "@/lib/server";
import { credits, relative, score } from "@/lib/format";
import { marketHref, tradeWords, type Profile } from "@/lib/social";
import { Photo } from "@/components/Photo";

export const dynamic = "force-dynamic";

export async function generateMetadata({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params;
  return { title: decodeURIComponent(name) };
}

function Stat({ label, value, tone }: { label: string; value: string; tone?: "yes" | "no" }) {
  return (
    <div className="stat">
      <span className="muted">{label}</span>
      <b className={tone}>{value}</b>
    </div>
  );
}

export default async function UserProfile({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params;
  const r = await gateway<Profile>(`/v1/users/${encodeURIComponent(decodeURIComponent(name))}`);
  if (!r.ok) notFound();
  const p = r.body;
  const now = renderTime();
  const initial = p.username.slice(0, 1).toUpperCase();
  return (
    <div className="profile stack" style={{ gap: 20 }}>
      <header className="profile-head">
        <span className="avatar" aria-hidden="true">
          {initial}
        </span>
        <div>
          <h1 className="display">{p.username}</h1>
          <p className="muted" style={{ margin: 0 }}>
            {p.joined ? `Joined ${relative(p.joined, now)}` : "Member"}
            {p.role === "trader" ? "" : ` · ${p.role === "admin" ? "Brier staff" : "Brier service account"}`}
          </p>
        </div>
      </header>

      <section className="stats card" aria-label="Record">
        <Stat label="Profit (settled)" value={`${p.profit > 0 ? "+" : ""}${credits(p.profit)}`} tone={p.profit > 0 ? "yes" : p.profit < 0 ? "no" : undefined} />
        <Stat label="Forecasting skill" value={p.average_skill == null ? "—" : score(p.average_skill)} tone={p.average_skill == null ? undefined : p.average_skill >= 0 ? "yes" : "no"} />
        <Stat label="Markets traded" value={p.markets_traded.toLocaleString("en-US")} />
        <Stat label="Contracts traded" value={p.volume.toLocaleString("en-US")} />
      </section>

      <div className="grid-2" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(min(100%, 420px), 1fr))", alignItems: "start" }}>
        <section className="card table-scroll" aria-labelledby="pos-h">
          <h2 id="pos-h" className="card-title">Open positions</h2>
          {p.positions.length === 0 ? (
            <p className="empty">No open positions.</p>
          ) : (
            <ul className="plist">
              {p.positions.map((x) => (
                <li key={x.market}>
                  <Photo of={{ title: x.title }} size="sm" />
                  <Link href={marketHref(x)} className="what">
                    <span className="t">{x.title ?? `Market ${x.market}`}</span>
                    {x.outcome && <span className="muted">{x.outcome}</span>}
                  </Link>
                  <span className={`chip ${x.position > 0 ? "ok" : "bad"}`}>
                    {Math.abs(x.position).toLocaleString("en-US")} {x.position > 0 ? "Yes" : "No"}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="card table-scroll" aria-labelledby="trades-h">
          <h2 id="trades-h" className="card-title">Recent trades</h2>
          {p.trades.length === 0 ? (
            <p className="empty">No trades yet.</p>
          ) : (
            <ul className="plist">
              {p.trades.map((t) => {
                const w = tradeWords(t.side, t.price);
                return (
                  <li key={`${t.seq}-${t.market}-${t.price}-${t.qty}`}>
                    <Photo of={{ title: t.title }} size="sm" />
                    <Link href={marketHref(t)} className="what">
                      <span className="t">{t.title ?? `Market ${t.market}`}</span>
                      <span className="muted">
                        Bought {t.qty.toLocaleString("en-US")} {w.word} at {w.price}¢{t.at ? ` · ${relative(t.at, now)}` : ""}
                      </span>
                    </Link>
                    <span className={w.word === "Yes" ? "yes" : "no"} style={{ fontWeight: 650, whiteSpace: "nowrap" }}>
                      {credits(t.qty * w.price)}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}
