"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { api } from "@/lib/client";
import { relative } from "@/lib/format";
import { displayName, tradeWords, type Activity, type Holders } from "@/lib/social";

/** Refresh the panel this often while it is open: activity is the read model, not the feed. */
const REFRESH_MS = 15_000;

function Who({ name }: { name: string }) {
  const shown = displayName(name);
  if (shown !== name) return <span className="trader muted">{shown}</span>;
  return (
    <Link className="trader" href={`/u/${encodeURIComponent(name)}`}>
      {name}
    </Link>
  );
}

/** Recent trades and the biggest holders of one market, as tabs. */
export function MarketSocial({ market }: { market: number }) {
  const [tab, setTab] = useState<"activity" | "holders">("activity");
  const [activity, setActivity] = useState<Activity | null>(null);
  const [holders, setHolders] = useState<Holders | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const [a, h] = await Promise.all([
          api<Activity>("GET", `/v1/markets/${market}/activity?limit=20`),
          api<Holders>("GET", `/v1/markets/${market}/holders`),
        ]);
        if (alive) {
          setActivity(a);
          setHolders(h);
          setNow(Date.now());
        }
      } catch {
        // Keep what is shown; the next refresh tries again.
      }
    };
    void load();
    const timer = setInterval(load, REFRESH_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [market]);

  return (
    <section className="card social-card" aria-label="Activity and holders">
      <div className="tabs" role="tablist" aria-label="Market activity">
        <button type="button" role="tab" id="tab-activity" aria-controls="panel-activity" aria-selected={tab === "activity"} onClick={() => setTab("activity")}>
          Activity
        </button>
        <button type="button" role="tab" id="tab-holders" aria-controls="panel-holders" aria-selected={tab === "holders"} onClick={() => setTab("holders")}>
          Top holders
        </button>
      </div>
      {tab === "activity" ? (
        <div role="tabpanel" id="panel-activity" aria-labelledby="tab-activity">
          {!activity ? (
            <p className="empty">Loading…</p>
          ) : activity.trades.length === 0 ? (
            <p className="empty">No trades yet. Be the first.</p>
          ) : (
            <ul className="feed">
              {activity.trades.map((t, i) => {
                const w = tradeWords(t.side, t.price);
                return (
                  <li key={`${t.seq}-${i}`}>
                    <span className="avatar sm" aria-hidden="true">
                      {displayName(t.username).slice(0, 1).toUpperCase()}
                    </span>
                    <span>
                      <Who name={t.username} /> bought <b className={w.word === "Yes" ? "yes" : "no"}>{t.qty.toLocaleString("en-US")} {w.word}</b> at {w.price}¢
                    </span>
                    {t.at && <span className="when">{relative(t.at, now)}</span>}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : (
        <div role="tabpanel" id="panel-holders" aria-labelledby="tab-holders">
          {!holders ? (
            <p className="empty">Loading…</p>
          ) : (
            <div className="holders">
              {(["yes", "no"] as const).map((side) => (
                <div key={side}>
                  <h3 className={side}>{side === "yes" ? "Yes holders" : "No holders"}</h3>
                  {holders[side].length === 0 ? (
                    <p className="muted" style={{ margin: "6px 0", fontSize: 14 }}>
                      None yet.
                    </p>
                  ) : (
                    <ol>
                      {holders[side].map((h) => (
                        <li key={h.username}>
                          <Who name={h.username} />
                          <span>{h.contracts.toLocaleString("en-US")}</span>
                        </li>
                      ))}
                    </ol>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
