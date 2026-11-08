"use client";
import { useEffect, useMemo, useRef, useState } from "react";

const RANGES = [
  ["1H", 3600],
  ["1D", 86_400],
  ["1W", 7 * 86_400],
  ["ALL", Infinity],
] as const;

/** Price history (percent) with range tabs and a pointer readout. The figure's label carries a
 * text summary, so the chart is not the only way to read the trend. */
export function PriceChart({ points }: { points: [string, number][] }) {
  const [range, setRange] = useState<(typeof RANGES)[number][0]>("ALL");
  const [hover, setHover] = useState<number | null>(null);
  // Draw at the container's real width, so text and lines are never stretched.
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(760);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => setWidth(Math.max(280, Math.round(entries[0]!.contentRect.width))));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const shown = useMemo(() => {
    if (points.length === 0) return points;
    const end = Date.parse(points[points.length - 1]![0]);
    const span = RANGES.find((r) => r[0] === range)![1] * 1000;
    const cut = points.filter((p) => end - Date.parse(p[0]) <= span);
    return cut.length >= 2 ? cut : points.slice(-2);
  }, [points, range]);

  const W = width;
  const H = width < 500 ? 220 : 300;
  const pad = { l: 8, r: 44, t: 14, b: 26 };
  if (points.length < 2) {
    return (
      <div className="empty" style={{ height: 220, display: "grid", placeItems: "center" }}>
        The price is recorded every minute once trading opens; the chart starts with the first two minutes.
      </div>
    );
  }
  const values = shown.map((p) => p[1]);
  const lo = Math.max(0, Math.floor((Math.min(...values) - 5) / 10) * 10);
  const hi = Math.min(100, Math.ceil((Math.max(...values) + 5) / 10) * 10);
  const t0 = Date.parse(shown[0]![0]);
  const t1 = Date.parse(shown[shown.length - 1]![0]);
  const x = (t: number) => pad.l + ((t - t0) / Math.max(1, t1 - t0)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + (1 - (v - lo) / Math.max(1, hi - lo)) * (H - pad.t - pad.b);
  const path = shown.map(([t, v], i) => `${i ? "L" : "M"}${x(Date.parse(t)).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const ticks = Array.from({ length: (hi - lo) / 10 + 1 }, (_, i) => lo + i * 10).filter((v, i, a) => a.length <= 6 || i % 2 === 0);
  const labels = [0, 0.5, 1].map((f) => t0 + f * (t1 - t0));
  const fmt = (t: number) =>
    new Date(t).toLocaleString("en-US", t1 - t0 > 2 * 86_400_000 ? { month: "short", day: "numeric", timeZone: "UTC" } : { hour: "numeric", minute: "2-digit", timeZone: "UTC" });
  const first = values[0]!;
  const last = values[values.length - 1]!;
  const summary = `Chance moved from ${first}% to ${last}% over this range (low ${Math.min(...values)}%, high ${Math.max(...values)}%).`;
  const h = hover === null ? null : shown[hover];

  return (
    <div className="stack" style={{ gap: 8 }}>
      <div className="chart-wrap" ref={box}>
        <svg
          viewBox={`0 0 ${W} ${H}`}
          style={{ height: H }}
          role="img"
          aria-label={summary}
          onMouseMove={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            const t = t0 + (((e.clientX - r.left) / r.width) * W - pad.l) / (W - pad.l - pad.r) * (t1 - t0);
            let best = 0;
            shown.forEach((p, i) => {
              if (Math.abs(Date.parse(p[0]) - t) < Math.abs(Date.parse(shown[best]![0]) - t)) best = i;
            });
            setHover(best);
          }}
          onMouseLeave={() => setHover(null)}
        >
          <defs>
            <linearGradient id="area" x1="0" x2="0" y1="0" y2="1">
              <stop offset="0" stopColor="var(--yes)" stopOpacity="0.22" />
              <stop offset="1" stopColor="var(--yes)" stopOpacity="0" />
            </linearGradient>
          </defs>
          {ticks.map((v) => (
            <g key={v}>
              <line className="grid-line" x1={pad.l} x2={W - pad.r} y1={y(v)} y2={y(v)} />
              <text className="axis" x={W - pad.r + 8} y={y(v) + 4}>
                {v}%
              </text>
            </g>
          ))}
          {labels.map((t, i) => (
            <text key={i} className="axis" x={x(t)} y={H - 6} textAnchor={i === 0 ? "start" : i === 2 ? "end" : "middle"}>
              {fmt(t)}
            </text>
          ))}
          <path d={`${path}L${x(t1)},${H - pad.b}L${x(t0)},${H - pad.b}Z`} fill="url(#area)" />
          <path className="line" d={path} />
          <circle cx={x(t1)} cy={y(last)} r="4" fill="var(--yes)" />
          {h && <line className="cursor" x1={x(Date.parse(h[0]))} x2={x(Date.parse(h[0]))} y1={pad.t} y2={H - pad.b} />}
        </svg>
        {h && (
          <div className="chart-tip" style={{ left: `min(calc(${(x(Date.parse(h[0])) / W) * 100}% + 8px), calc(100% - 150px))` }}>
            <strong>{h[1]}%</strong> · {new Date(h[0]).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" })} UTC
          </div>
        )}
      </div>
      <div className="spread">
        <span className="muted" style={{ fontSize: 13 }}>
          {summary}
        </span>
        <div className="range" role="group" aria-label="Chart range">
          {RANGES.map(([name]) => (
            <button key={name} type="button" aria-pressed={range === name} onClick={() => setRange(name)}>
              {name}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
