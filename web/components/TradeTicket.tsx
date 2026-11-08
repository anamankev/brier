"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError, clientOrderId } from "@/lib/client";
import { credits } from "@/lib/format";

type Result = { status: string; filled: number; remaining: number; cancel_reason: string | null; order: number };

const REASONS: Record<string, string> = {
  insufficient_collateral: "Not enough available credits for this order.",
  market_not_open: "The market is not open for trading.",
  invalid_price: "Prices are 1 to 99.",
  invalid_quantity: "Quantity is 1 to 25,000 contracts.",
  post_only_would_cross: "A post-only order would have traded at once.",
  fok_unfillable: "There is not enough on the book to fill this order completely.",
  too_many_orders: "You have the most resting orders allowed.",
  rate_limited: "Too many orders at once; try again in a moment.",
};

/** Buy YES or buy NO. Buying NO at q is entered as selling YES at 100 − q (SPEC §2.2). */
export function TradeTicket({ market }: { market: number }) {
  const router = useRouter();
  const [outcome, setOutcome] = useState<"yes" | "no">("yes");
  const [price, setPrice] = useState(50);
  const [qty, setQty] = useState(10);
  const [tif, setTif] = useState<"gtc" | "ioc" | "fok">("gtc");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const cost = qty * price;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setMessage(null);
    const order = {
      market,
      side: outcome === "yes" ? "buy" : "sell",
      price: outcome === "yes" ? price : 100 - price,
      qty,
      tif,
      client_order_id: clientOrderId(),
    };
    try {
      const r = await api<Result>("POST", "/v1/orders", order);
      const what = `${r.filled} of ${qty} filled`;
      setMessage({
        ok: true,
        text: r.status === "resting" ? `${what}; ${r.remaining} resting as order ${r.order}.` : r.status === "cancelled" ? `${what}; the rest was cancelled (${r.cancel_reason}).` : `${what}.`,
      });
      router.refresh();
    } catch (err) {
      const code = err instanceof ApiError ? err.code : "";
      setMessage({ ok: false, text: REASONS[code] ?? (err instanceof Error ? err.message : "The order failed.") });
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="stack" aria-label="Trade ticket">
      <fieldset style={{ border: 0, padding: 0, margin: 0 }}>
        <legend style={{ fontWeight: 650, marginBottom: 6 }}>Buy</legend>
        <div className="row">
          <button type="button" className="buy-yes" aria-pressed={outcome === "yes"} onClick={() => setOutcome("yes")}>
            YES
          </button>
          <button type="button" className="buy-no" aria-pressed={outcome === "no"} onClick={() => setOutcome("no")}>
            NO
          </button>
        </div>
      </fieldset>
      <label>
        Limit price for {outcome.toUpperCase()} (¢, 1–99)
        <input type="number" min={1} max={99} required value={price} onChange={(e) => setPrice(Number(e.target.value))} />
      </label>
      <label>
        Contracts
        <input type="number" min={1} max={25000} required value={qty} onChange={(e) => setQty(Number(e.target.value))} />
      </label>
      <label>
        Time in force
        <select value={tif} onChange={(e) => setTif(e.target.value as typeof tif)}>
          <option value="gtc">Good till cancelled (rests in the book)</option>
          <option value="ioc">Immediate or cancel</option>
          <option value="fok">Fill or kill</option>
        </select>
      </label>
      <p className="muted" style={{ margin: 0 }}>
        Costs at most {credits(cost)} credits; pays {credits(qty * 100)} if {outcome.toUpperCase()} wins.
      </p>
      <button className="primary" type="submit" disabled={busy}>
        {busy ? "Placing…" : `Buy ${qty} ${outcome.toUpperCase()} at ${price}¢`}
      </button>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
