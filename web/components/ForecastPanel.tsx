"use client";
import { useEffect, useState } from "react";
import { api, ApiError } from "@/lib/client";

type Forecast = { market: number; probability: number; at: string };

/** Record a probability without trading. Scored against the market when it resolves. */
export function ForecastPanel({ market, open }: { market: number; open: boolean }) {
  const [current, setCurrent] = useState<Forecast | null>(null);
  const [value, setValue] = useState(50);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    api<{ forecasts: Forecast[] }>("GET", "/v1/forecasts")
      .then((r) => {
        const latest = r.forecasts.find((f) => f.market === market) ?? null;
        setCurrent(latest);
        if (latest) setValue(latest.probability);
      })
      .catch(() => {});
  }, [market]);

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
    <form onSubmit={submit} className="stack" aria-label="Your forecast">
      <p style={{ margin: 0 }}>
        {current ? (
          <>
            Your forecast: <strong>{current.probability}%</strong>
          </>
        ) : (
          "You have not forecast this market."
        )}
      </p>
      <label>
        Your probability of YES: {value}%
        <input type="range" min={1} max={99} value={value} onChange={(e) => setValue(Number(e.target.value))} disabled={!open} />
      </label>
      <button type="submit" disabled={!open}>
        {open ? "Save forecast" : "Forecasts are closed"}
      </button>
      <p className="muted" style={{ margin: 0, fontSize: "0.85rem" }}>
        Scored with the Brier score against the market price over the whole time the market is open: an early, right
        forecast scores best.
      </p>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
