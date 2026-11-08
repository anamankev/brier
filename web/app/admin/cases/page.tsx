import Link from "next/link";
import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { when } from "@/lib/format";
import { TYPOLOGY, scoreText, type Case } from "@/lib/cases";

export const dynamic = "force-dynamic";
export const metadata = { title: "Surveillance cases" };

const STATUSES = ["open", "dismissed", "actioned", "all"] as const;

export default async function Cases({ searchParams }: { searchParams: Promise<{ status?: string }> }) {
  const user = await me();
  if (!user) redirect("/signin");
  if (user.role !== "admin" || !user.signed_in_with_totp) redirect("/admin");
  const raw = (await searchParams).status;
  const status = STATUSES.find((s) => s === raw) ?? "open";
  const res = await gateway<{ cases: Case[] }>(`/v1/admin/cases?status=${status}`);
  const cases = res.ok ? res.body.cases : [];
  return (
    <div className="stack" style={{ gap: 20 }}>
      <div>
        <p className="kicker"><Link href="/admin">Admin</Link></p>
        <h1 className="display">Surveillance cases</h1>
        <p className="muted">
          Each case is an alert from a detector, not a finding. Read the evidence, then close it as dismissed or actioned with a reason.
        </p>
      </div>
      <nav className="tabs" aria-label="Case status">
        {STATUSES.map((s) => (
          <Link key={s} href={`/admin/cases?status=${s}`} aria-current={s === status ? "page" : undefined}>
            {s.charAt(0).toUpperCase() + s.slice(1)}
          </Link>
        ))}
      </nav>
      {!res.ok && <p className="notice error">Cases could not be loaded: {res.body.error?.message ?? "the exchange is not reachable"}.</p>}
      <div className="card table-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">Case</th>
              <th scope="col">Typology</th>
              <th scope="col">Market</th>
              <th scope="col">Account</th>
              <th scope="col" className="num">Score</th>
              <th scope="col">Opened</th>
              <th scope="col">Status</th>
            </tr>
          </thead>
          <tbody>
            {cases.map((c) => (
              <tr key={c.case}>
                <td><Link href={`/admin/cases/${c.case}`}>#{c.case}</Link></td>
                <td>{TYPOLOGY[c.typology].name}</td>
                <td>{c.market_title ?? `Market ${c.market}`}</td>
                <td>{c.username ?? `account ${c.account}`}</td>
                <td className="num mono">{scoreText(c)}</td>
                <td>{when(c.opened_at)}</td>
                <td>{c.status}</td>
              </tr>
            ))}
            {cases.length === 0 && (
              <tr>
                <td colSpan={7} className="muted">No {status === "all" ? "" : `${status} `}cases.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
