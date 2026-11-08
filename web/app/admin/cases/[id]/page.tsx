import Link from "next/link";
import { notFound, redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { credits, when } from "@/lib/format";
import { TYPOLOGY, scoreText, evidenceSeqs, type CaseDetail } from "@/lib/cases";
import { CloseCase } from "./CloseCase";

export const dynamic = "force-dynamic";
export const metadata = { title: "Surveillance case" };

const LISTS = new Set(["timeline", "pnl", "orders", "trade_seqs", "counterparties", "close_clock"]);

const LABELS: Record<string, string> = {
  linked_contracts: "Contracts traded with linked accounts",
  contracts: "Contracts traded in this market",
  median_gtc: "Median resting order size",
  taker_contracts: "Contracts it bought or sold aggressively before the close",
  window_taker_contracts: "All aggressive contracts before the close",
  ticks_moved: "Last price moved by its orders (ticks)",
  winning_contracts: "Position added on the winning side before the close",
  earlier_contracts: "Contracts traded before that",
  opened_after_market: "Account opened after the market",
  linked: "Shares a login address",
};

function label(key: string): string {
  return LABELS[key] ?? key.replaceAll("_", " ").replace(/^./, (c) => c.toUpperCase());
}

function shown(v: unknown): string {
  if (typeof v === "boolean") return v ? "yes" : "no";
  return String(v);
}

function Row({ entry, flagged }: { entry: unknown[]; flagged: boolean }) {
  const [seq, kind] = entry as [number, string];
  let what = "";
  let detail = "";
  if (kind === "place") {
    const [, , side, price, qty, tif, bid, ask] = entry as [number, string, string, number, number, string, number | null, number | null];
    what = `Placed ${side} ${qty} at ${price}¢ (${tif.toUpperCase()})`;
    detail = `book before: ${bid ?? "–"}¢ / ${ask ?? "–"}¢`;
  } else if (kind === "fill") {
    const [, , side, price, qty, role, other] = entry as [number, string, string, number, number, string, number];
    what = `${side === "buy" ? "Bought" : "Sold"} ${qty} at ${price}¢`;
    detail = `${role}, against account ${other}`;
  } else if (kind === "cancel") {
    const [, , orderSeq, reason] = entry as [number, string, number, string];
    what =
      reason === "ioc_remainder"
        ? `Unfilled rest of #${orderSeq} cancelled`
        : `Cancelled the order placed at #${orderSeq}`;
    detail = { ioc_remainder: "immediate-or-cancel", user: "by the account", market_closed: "market closed", kill_switch: "kill switch", self_trade: "self-trade prevention" }[reason] ?? reason.replaceAll("_", " ");
  }
  return (
    <tr className={flagged ? "flagged" : undefined}>
      <td className="num mono">#{seq}</td>
      <td>{what}{flagged && <span className="sr-only"> (named in the alert)</span>}</td>
      <td className="muted">{detail}</td>
    </tr>
  );
}

export default async function CasePage({ params }: { params: Promise<{ id: string }> }) {
  const user = await me();
  if (!user) redirect("/signin");
  if (user.role !== "admin" || !user.signed_in_with_totp) redirect("/admin");
  const { id } = await params;
  if (!/^\d+$/.test(id)) notFound();
  const res = await gateway<CaseDetail>(`/v1/admin/cases/${id}`);
  if (!res.ok) {
    if (res.status === 404) notFound();
    return <p className="notice error">This case could not be loaded: {res.body.error?.message ?? "the exchange is not reachable"}.</p>;
  }
  const c = res.body;
  const t = TYPOLOGY[c.typology];
  const flagged = evidenceSeqs(c.evidence);
  const timeline = (c.evidence.timeline ?? []) as unknown[][];
  const facts = Object.entries(c.evidence).filter(([k, v]) => !LISTS.has(k) && (typeof v === "number" || typeof v === "boolean" || typeof v === "string"));
  const pnl = c.evidence.pnl;
  return (
    <div className="stack" style={{ gap: 20 }}>
      <div>
        <p className="kicker"><Link href="/admin/cases">Surveillance cases</Link></p>
        <h1 className="display">Case #{c.case}: {t.name}</h1>
        <p className="muted">{t.what}</p>
      </div>
      <div className="stats">
        <div className="stat card"><div className="label">Account</div><div className="value small">{c.username ?? `account ${c.account}`}</div></div>
        <div className="stat card"><div className="label">Market</div><div className="value small"><Link href={`/markets/${c.market}`}>{c.market_title ?? `Market ${c.market}`}</Link></div></div>
        <div className="stat card"><div className="label">Score</div><div className="value small">{scoreText(c)}</div></div>
        {pnl && (
          <div className="stat card">
            <div className="label">P&amp;L here, {pnl.valued_at === "last trade" ? "at last trade" : `settled ${pnl.valued_at.toUpperCase()}`}</div>
            <div className="value small">{credits(pnl.value)}</div>
          </div>
        )}
      </div>
      <section className="card pad" aria-labelledby="facts-h">
        <h2 id="facts-h" style={{ fontSize: 18, marginBottom: 10 }}>What the detector measured</h2>
        <dl className="facts">
          {facts.map(([k, v]) => (
            <div key={k}><dt>{label(k)}</dt><dd className="mono">{shown(v)}</dd></div>
          ))}
          {Array.isArray(c.evidence.orders) && (
            <div>
              <dt>Large orders pulled unfilled</dt>
              <dd className="mono">
                {c.evidence.orders.length}, of which{" "}
                {(c.evidence.orders as { against_seqs?: number[] }[]).filter((o) => (o.against_seqs ?? []).length > 0).length} around its own trade the other way
              </dd>
            </div>
          )}
          {pnl && <div><dt>Position at the end</dt><dd className="mono">{pnl.position} contracts</dd></div>}
          <div><dt>Detector</dt><dd>{c.detector}</dd></div>
          <div><dt>Opened</dt><dd>{when(c.opened_at)}</dd></div>
        </dl>
      </section>
      <section className="card pad" aria-labelledby="linked-h">
        <h2 id="linked-h" style={{ fontSize: 18, marginBottom: 10 }}>Accounts sharing a login address</h2>
        {c.linked_accounts.length ? (
          <ul className="list card" style={{ listStyle: "none", padding: 0 }}>{c.linked_accounts.map((l) => <li key={l.account}>{l.username} <span className="muted">(account {l.account})</span></li>)}</ul>
        ) : (
          <p className="muted">None recorded. A shared address is weak evidence either way: households share one, and VPNs hide one.</p>
        )}
      </section>
      <section className="card pad" aria-labelledby="timeline-h">
        <h2 id="timeline-h" style={{ fontSize: 18, marginBottom: 4 }}>Activity in this market</h2>
        <p className="muted" style={{ marginBottom: 10 }}>By journal sequence number. Highlighted rows are the ones the alert names.</p>
        <div className="table-scroll">
          <table>
            <thead><tr><th scope="col" className="num">Seq</th><th scope="col">Event</th><th scope="col">Detail</th></tr></thead>
            <tbody>
              {timeline.map((e, i) => <Row key={i} entry={e} flagged={flagged.has(e[0] as number)} />)}
              {timeline.length === 0 && <tr><td colSpan={3} className="muted">No activity was attached.</td></tr>}
            </tbody>
          </table>
        </div>
      </section>
      {c.status === "open" ? (
        <CloseCase id={c.case} />
      ) : (
        <p className="notice ok">
          Closed as {c.status} by {c.closed_by} on {c.closed_at ? when(c.closed_at) : ""}: {c.close_reason}
        </p>
      )}
    </div>
  );
}
