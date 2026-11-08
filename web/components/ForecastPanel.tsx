"use client";
import { useState } from "react";
import { api, ApiError } from "@/lib/client";

type Forecast = { market: number; probability: number; at: string };

/** Record a probability without trading; scored against the market price when it resolves. */
export function ForecastPanel({ market, open, marketChance, initial }: { market: number; open: boolean; marketChance: number | null; initial: Forecast | null }) {
  const [current, setCurrent] = useState<Forecast | null>(initial);
  const [value, setValue] = useState(initial?.probability ?? marketChance ?? 50);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    try {
      const f = await api<Forecast>("PUT", `/v1/markets/${market}/forecast`, { probability: value });
      setCurrent(f);
      setMessage({ ok: true, text: `Recorded: ${f.probability}%.` });
    } catch (err) {
      setMessage({ ok: false, text: err instanceof ApiError ? err.message : "Could not record the forecast." });
    }
  }

  return (
    <form className="card panel" onSubmit={submit} aria-labelledby="forecast-h">
      <div className="spread">
        <h2 id="forecast-h" style={{ fontSize: 16 }}>
          Your forecast
        </h2>
        <span className="chip">No credits needed</span>
      </div>
      <div className="stats" style={{ gridTemplateColumns: "1fr 1fr", gap: 10 }}>
        <div>
          <div className="kicker">You</div>
          <div className="big-number" style={{ fontSize: 34 }}>
            {value}%
          </div>
        </div>
        <div>
          <div className="kicker">Market</div>
          <div className="big-number dim" style={{ fontSize: 34 }}>
            {marketChance == null ? "—" : `${marketChance}%`}
          </div>
        </div>
      </div>
      <label className="field">
        <span className="sr-only">Your probability of YES</span>
        <input type="range" min={1} max={99} value={value} onChange={(e) => setValue(Number(e.target.value))} disabled={!open} aria-label="Your probability of YES" />
      </label>
      <button type="submit" className="btn" disabled={!open}>
        {!open ? "Forecasts are closed" : current ? "Update forecast" : "Save forecast"}
      </button>
      <p className="muted" style={{ margin: 0, fontSize: 13 }}>
        Scored with the Brier score against the market price for the whole time trading is open. The earlier and closer
        you are, the more skill you earn.
      </p>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
