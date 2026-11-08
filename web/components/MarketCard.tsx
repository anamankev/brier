import Link from "next/link";
import type { Summary } from "@/lib/markets";
import { cents, chance, multiple, shortDate } from "@/lib/format";
import { Photo } from "./Photo";
import { Delta } from "./Delta";
import { Sparkline } from "./Sparkline";

const STATE: Record<string, string> = {
  pre_open: "Opens soon",
  open: "",
  halted: "Halted",
  closed: "Awaiting result",
  resolved: "Resolved",
  voided: "Voided",
};

export function MarketCard({ m }: { m: Summary }) {
  const href = `/markets/${m.market}`;
  const status = STATE[m.state];
  return (
    <article className="card mcard" aria-labelledby={`m${m.market}`}>
      <div className="head">
        <Photo of={m} />
        <div>
          <Link id={`m${m.market}`} href={href} className="title">
            {m.title}
          </Link>
          <div className="meta">
            {status ? <span className="chip">{status}</span> : <>Trading closes {shortDate(m.close ?? m.release)}</>}
          </div>
        </div>
      </div>
      <div className="outcome">
        <div className="row" style={{ gap: 14 }}>
          <Sparkline points={m.points} width={120} height={40} />
          <div>
            <div className="mult">{m.chance == null ? "No trades yet" : `Yes pays ${multiple(m.yesPrice ?? m.chance)}`}</div>
            <Delta change={m.change} suffix=" today" />
          </div>
        </div>
        <span className={`chance-pill${m.chance == null ? " none" : ""}`} aria-label={`${chance(m.chance)} chance`}>
          {chance(m.chance)}
        </span>
      </div>
      {m.state === "open" && (
        <div className="seg">
          <Link className="px yes" href={`${href}?buy=yes`}>
            Yes {cents(m.yesPrice)}
          </Link>
          <Link className="px no" href={`${href}?buy=no`}>
            No {cents(m.noPrice)}
          </Link>
        </div>
      )}
      <div className="foot">
        <span>{(m.volume ?? 0).toLocaleString("en-US")} contracts traded</span>
        {m.rulesPinned && <span className="chip ok">Rules pinned</span>}
      </div>
    </article>
  );
}
