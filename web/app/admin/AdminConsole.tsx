"use client";
import { useCallback, useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";
import { when } from "@/lib/format";

export type QueueResolution = {
  resolution: number;
  market: number;
  title: string | null;
  outcome: string;
  void_condition: string | null;
  state: string;
  yours: boolean;
  proposed_by: string;
  dispute_until: string | null;
  evidence_hash: string;
};
export type QueueDispute = { dispute: number; resolution: number; market: number; reason: string; filed_at: string; yours: boolean };
export type Entry = { id: number; at: string; actor: string; username: string | null; action: string; reason: string; seq: number | null };

function useNotice() {
  const [notice, setNotice] = useState<{ ok: boolean; text: string } | null>(null);
  const run = useCallback(async (what: string, f: () => Promise<unknown>) => {
    try {
      await f();
      setNotice({ ok: true, text: `${what}: done.` });
      return true;
    } catch (err) {
      setNotice({ ok: false, text: `${what}: ${err instanceof ApiError ? err.message : "failed"}` });
      return false;
    }
  }, []);
  return { notice, run };
}

function Reason({ id, value, onChange }: { id: string; value: string; onChange: (v: string) => void }) {
  return (
    <label htmlFor={id} className="field">
      <span>Reason (3–500 characters)</span>
      <input id={id} value={value} onChange={(e) => onChange(e.target.value)} minLength={3} maxLength={500} required />
    </label>
  );
}

export function AdminConsole({
  queue,
  audit,
}: {
  queue: { resolutions: QueueResolution[]; disputes: QueueDispute[] };
  audit: Entry[];
}) {
  const router = useRouter();
  const [reason, setReason] = useState("");
  const [rulebook, setRulebook] = useState("");
  const [stateMarket, setStateMarket] = useState("");
  const [newState, setNewState] = useState("halted");
  const { notice, run } = useNotice();
  const reload = () => router.refresh();

  const act = async (what: string, method: string, path: string, body: Record<string, unknown>) => {
    if (await run(what, () => api(method, path, { ...body, reason }))) reload();
  };

  return (
    <>
      <div aria-live="polite">{notice && <p className={`notice ${notice.ok ? "ok" : "error"}`}>{notice.text}</p>}</div>
      <section className="card pad" aria-labelledby="reason-h">
        <h2 id="reason-h" style={{ fontSize: 18, marginBottom: 10 }}>Reason for the next action</h2>
        <Reason id="reason" value={reason} onChange={setReason} />
      </section>

      <section className="card pad" aria-labelledby="queue-h">
        <h2 id="queue-h" style={{ fontSize: 18, marginBottom: 10 }}>Resolutions waiting</h2>
        <table>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Outcome</th>
              <th scope="col">State</th>
              <th scope="col">Proposed by</th>
              <th scope="col">Actions</th>
            </tr>
          </thead>
          <tbody>
            {queue.resolutions.map((r) => (
              <tr key={r.resolution}>
                <td>
                  <a href={`/markets/${r.market}`}>{r.title ?? `Market ${r.market}`}</a>
                </td>
                <td>
                  {r.outcome.toUpperCase()}
                  {r.void_condition && ` (${r.void_condition.replaceAll("_", " ")})`}
                </td>
                <td>
                  {r.state}
                  {r.dispute_until && r.state === "confirmed" && <div className="muted">disputes until {when(r.dispute_until)}</div>}
                </td>
                <td>{r.yours ? "you (another admin must confirm)" : r.proposed_by}</td>
                <td className="row">
                  {r.state === "proposed" && !r.yours && (
                    <button type="button" className="btn primary small" onClick={() => act(`Confirm resolution ${r.resolution}`, "POST", `/v1/admin/resolutions/${r.resolution}/confirm`, {})}>
                      Confirm
                    </button>
                  )}
                  {(r.state === "proposed" || r.state === "confirmed") && (
                    <button type="button" className="btn small" onClick={() => act(`Withdraw resolution ${r.resolution}`, "POST", `/v1/admin/resolutions/${r.resolution}/withdraw`, {})}>
                      Withdraw
                    </button>
                  )}
                </td>
              </tr>
            ))}
            {queue.resolutions.length === 0 && (
              <tr>
                <td colSpan={5} className="muted">
                  Nothing waiting.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <section className="card pad" aria-labelledby="disputes-h">
        <h2 id="disputes-h" style={{ fontSize: 18, marginBottom: 10 }}>Open disputes</h2>
        <table>
          <thead>
            <tr>
              <th scope="col">Market</th>
              <th scope="col">Filed</th>
              <th scope="col">Reason given</th>
              <th scope="col">Ruling</th>
            </tr>
          </thead>
          <tbody>
            {queue.disputes.map((d) => (
              <tr key={d.dispute}>
                <td>
                  <a href={`/markets/${d.market}`}>Market {d.market}</a>
                </td>
                <td>{when(d.filed_at)}</td>
                <td>{d.reason}</td>
                <td className="row">
                  {d.yours ? (
                    <span className="muted">Your proposal: another admin rules</span>
                  ) : (
                    <>
                      <button type="button" className="btn small" onClick={() => act(`Uphold dispute ${d.dispute}`, "POST", `/v1/admin/disputes/${d.dispute}/rule`, { decision: "uphold" })}>
                        Uphold
                      </button>
                      <button type="button" className="btn small" onClick={() => act(`Reject dispute ${d.dispute}`, "POST", `/v1/admin/disputes/${d.dispute}/rule`, { decision: "reject" })}>
                        Reject
                      </button>
                    </>
                  )}
                </td>
              </tr>
            ))}
            {queue.disputes.length === 0 && (
              <tr>
                <td colSpan={4} className="muted">
                  No open disputes.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <section className="card pad" aria-labelledby="list-h">
        <h2 id="list-h" style={{ fontSize: 18, marginBottom: 10 }}>List a market</h2>
        <form
          className="stack"
          style={{ maxWidth: "none" }}
          onSubmit={async (e) => {
            e.preventDefault();
            let parsed: unknown;
            try {
              parsed = JSON.parse(rulebook);
            } catch {
              await run("List the market", async () => {
                throw new ApiError(400, "bad_request", "the rulebook is not valid JSON");
              });
              return;
            }
            await act("List the market", "POST", "/v1/admin/markets", { rulebook: parsed });
          }}
        >
          <label className="field">
            <span>Rulebook (JSON; the files in markets/ are TOML, see docs/rulebook.md)</span>
            <textarea rows={12} value={rulebook} onChange={(e) => setRulebook(e.target.value)} required />
          </label>
          <button className="btn primary" type="submit">
            List market
          </button>
        </form>
      </section>

      <section className="card pad" aria-labelledby="ops-h">
        <h2 id="ops-h" style={{ fontSize: 18, marginBottom: 10 }}>Market state and kill switches</h2>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            void act(`Set market ${stateMarket} to ${newState}`, "POST", `/v1/admin/markets/${Number(stateMarket)}/state`, { state: newState });
          }}
        >
          <label className="field">
            <span>Market ID</span>
            <input inputMode="numeric" pattern="[0-9]+" value={stateMarket} onChange={(e) => setStateMarket(e.target.value)} required />
          </label>
          <label className="field">
            <span>New state</span>
            <select value={newState} onChange={(e) => setNewState(e.target.value)}>
              <option value="halted">Halted</option>
              <option value="open">Open (inside its trading window only)</option>
              <option value="closed">Closed</option>
            </select>
          </label>
          <button type="submit" className="btn">Set state</button>
        </form>
        <div className="row" style={{ marginTop: 14 }}>
          <button type="button" className="btn danger" onClick={() => act("Halt every open market", "POST", "/v1/admin/halt-all", {})}>
            Halt all markets
          </button>
          <button type="button" className="btn danger" onClick={() => act("Cancel every resting order", "POST", "/v1/admin/cancel-all", {})}>
            Cancel all orders
          </button>
        </div>
      </section>

      <section className="card pad" aria-labelledby="audit-h">
        <h2 id="audit-h" style={{ fontSize: 18, marginBottom: 10 }}>Audit log (newest first)</h2>
        <table>
          <thead>
            <tr>
              <th scope="col">When</th>
              <th scope="col">Who</th>
              <th scope="col">Action</th>
              <th scope="col">Reason</th>
              <th scope="col" className="num">
                Seq
              </th>
            </tr>
          </thead>
          <tbody>
            {audit.map((e) => (
              <tr key={e.id}>
                <td>{when(e.at)}</td>
                <td>{e.actor === "system" ? "system" : e.username}</td>
                <td>{e.action}</td>
                <td>{e.reason}</td>
                <td className="num">{e.seq ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  );
}
