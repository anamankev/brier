import Link from "next/link";
import { cadence, compact, kind, label, leading, noPrice, yesPrice, type EventSummary } from "@/lib/events";
import { cents, relative } from "@/lib/format";
import { CategoryTile, Repeat } from "./Icons";
import { WatchButton } from "./WatchButton";

const STATUS: Record<string, string> = { upcoming: "Opens soon", closed: "Awaiting result", settled: "Settled" };

/** A chance arc for single-question events: the number is the content, the arc decoration. */
function Gauge({ value }: { value: number | null }) {
  const p = value ?? 0;
  const r = 22;
  const half = Math.PI * r;
  const tone = value == null ? "var(--line-2)" : p >= 50 ? "var(--yes)" : "var(--no)";
  return (
    <div className="gauge" aria-label={value == null ? "No price yet" : `${p}% chance`}>
      <svg width="60" height="34" viewBox="0 0 60 34" aria-hidden="true">
        <path d="M8 30a22 22 0 0 1 44 0" fill="none" stroke="var(--line-2)" strokeWidth="5" strokeLinecap="round" />
        {value != null && (
          <path d="M8 30a22 22 0 0 1 44 0" fill="none" stroke={tone} strokeWidth="5" strokeLinecap="round" strokeDasharray={`${(half * p) / 100} ${half}`} />
        )}
      </svg>
      <strong>{value == null ? "—" : `${p}%`}</strong>
      <span>chance</span>
    </div>
  );
}

export function EventCard({ e, now }: { e: EventSummary; now: number }) {
  const href = `/e/${e.slug}`;
  const single = e.markets.length === 1;
  const top = leading(e).slice(0, 2);
  const open = e.status === "open";
  const repeat = cadence(e);
  const quoted = e.markets.some((m) => m.best_bid != null || m.best_ask != null || m.chance != null);
  return (
    <article className="card ecard" aria-labelledby={`ev-${e.slug}`}>
      <header>
        <CategoryTile category={kind(e)} />
        <Link id={`ev-${e.slug}`} href={href} className={`title${single ? " long" : ""}`}>
          {e.title}
        </Link>
        {single && <Gauge value={e.markets[0]!.chance} />}
      </header>

      {single ? (
        open ? (
          <div className="seg big">
            <Link className="px yes" href={`${href}?m=${e.markets[0]!.market}&side=yes`}>
              Yes <span>{cents(yesPrice(e.markets[0]!))}</span>
            </Link>
            <Link className="px no" href={`${href}?m=${e.markets[0]!.market}&side=no`}>
              No <span>{cents(noPrice(e.markets[0]!))}</span>
            </Link>
          </div>
        ) : (
          <p className="muted state">{STATUS[e.status]}</p>
        )
      ) : open && !quoted ? (
        <p className="muted state">No quotes yet. The market maker quotes once its forecast covers this day.</p>
      ) : (
        <ul className="outcomes">
          {top.map((m) => (
            <li key={m.market}>
              <Link href={`${href}?m=${m.market}`} className="name">
                {label(m)}
              </Link>
              <span className="pct">{m.chance == null ? "—" : `${m.chance}%`}</span>
              {open ? (
                <span className="mini">
                  <Link className="yes" href={`${href}?m=${m.market}&side=yes`} aria-label={`Buy Yes on ${label(m)}`}>
                    Yes
                  </Link>
                  <Link className="no" href={`${href}?m=${m.market}&side=no`} aria-label={`Buy No on ${label(m)}`}>
                    No
                  </Link>
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      <footer>
        <span>{compact(e.volume)} traded</span>
        {repeat && (
          <span className="cad">
            <Repeat /> {repeat}
          </span>
        )}
        <span className="when">{open ? `closes ${relative(e.close, now)}` : (STATUS[e.status] ?? "")}</span>
        <WatchButton slug={e.slug} title={e.title} />
      </footer>
    </article>
  );
}
