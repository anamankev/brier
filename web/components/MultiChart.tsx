"use client";
import { useEffect, useMemo, useRef, useState } from "react";

export type Series = { name: string; points: [string, number][] };

// Distinct on the dark background and told apart by legend order and label, not colour alone.
export const LINE_COLOURS = ["#3ddc97", "#8fb4ff", "#f5b84d", "#c79bff"];

const RANGES = [
  ["6H", 6 * 3600],
  ["1D", 86_400],
  ["1W", 7 * 86_400],
  ["ALL", Infinity],
] as const;

/** Chances of several outcomes over time, with range tabs and a pointer readout. The figure's
 * label carries a text summary, so the lines are never the only way to read it. */
export function MultiChart({ series, height = 280 }: { series: Series[]; height?: number }) {
  const [range, setRange] = useState<(typeof RANGES)[number][0]>("ALL");
  const [hover, setHover] = useState<number | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(720);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => setWidth(Math.max(280, Math.round(entries[0]!.contentRect.width))));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const drawn = useMemo(() => {
    const all = series.filter((s) => s.points.length >= 2);
    const end = Math.max(0, ...all.map((s) => Date.parse(s.points[s.points.length - 1]![0])));
    const span = RANGES.find((r) => r[0] === range)![1] * 1000;
    return all.map((s) => {
      const cut = s.points.filter((p) => end - Date.parse(p[0]) <= span);
      return { ...s, points: cut.length >= 2 ? cut : s.points.slice(-2) };
    });
  }, [series, range]);

  const H = width < 420 ? Math.min(220, height) : height;
  const pad = { l: 8, r: 46, t: 14, b: 26 };
  if (drawn.length === 0) {
    return (
      <div ref={box} className="empty chart-empty" style={{ height: 200 }}>
        No price history yet. The chart fills in as soon as the market has a price.
      </div>
    );
  }
  const times = drawn.flatMap((s) => s.points.map((p) => Date.parse(p[0])));
  const t0 = Math.min(...times);
  const t1 = Math.max(...times);
  const values = drawn.flatMap((s) => s.points.map((p) => p[1]));
  const lo = Math.max(0, Math.floor((Math.min(...values) - 4) / 10) * 10);
  const hi = Math.min(100, Math.ceil((Math.max(...values) + 4) / 10) * 10);
  const W = width;
  const x = (t: number) => pad.l + ((t - t0) / Math.max(1, t1 - t0)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + (1 - (v - lo) / Math.max(1, hi - lo)) * (H - pad.t - pad.b);
  const ticks = Array.from({ length: (hi - lo) / 10 + 1 }, (_, i) => lo + i * 10).filter((_, i, a) => a.length <= 6 || i % 2 === 0);
  const fmt = (t: number) =>
    new Date(t).toLocaleString(
      "en-US",
      t1 - t0 > 2 * 86_400_000 ? { month: "short", day: "numeric", timeZone: "UTC" } : { hour: "numeric", minute: "2-digit", timeZone: "UTC" },
    );
  // The hover time snaps to the first series' nearest sample.
  const base = drawn[0]!.points;
  const at = hover === null ? null : Date.parse(base[hover]![0]);
  const valueAt = (s: Series, t: number) => {
    let best = s.points[0]!;
    for (const p of s.points) if (Date.parse(p[0]) <= t) best = p;
    return best[1];
  };
  const summary = drawn
    .map((s) => `${s.name}: from ${s.points[0]![1]}% to ${s.points[s.points.length - 1]![1]}%`)
    .join("; ");

  return (
    <div className="mchart">
      <div className="legend" aria-hidden="true">
        {drawn.map((s, i) => (
          <span key={s.name}>
            <i style={{ background: LINE_COLOURS[i % LINE_COLOURS.length] }} />
            {s.name} <b>{at === null ? s.points[s.points.length - 1]![1] : valueAt(s, at)}%</b>
          </span>
        ))}
      </div>
      <div ref={box} className="chart-wrap">
        <svg
          width={W}
          height={H}
          role="img"
          aria-label={`Chance over time. ${summary}.`}
          onPointerMove={(e) => {
            const r = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
            const t = t0 + ((e.clientX - r.left - pad.l) / Math.max(1, W - pad.l - pad.r)) * (t1 - t0);
            let best = 0;
            base.forEach((p, i) => {
              if (Math.abs(Date.parse(p[0]) - t) < Math.abs(Date.parse(base[best]![0]) - t)) best = i;
            });
            setHover(best);
          }}
          onPointerLeave={() => setHover(null)}
        >
          {ticks.map((v) => (
            <g key={v}>
              <line x1={pad.l} x2={W - pad.r} y1={y(v)} y2={y(v)} stroke="var(--line)" strokeDasharray="2 5" />
              <text x={W - pad.r + 8} y={y(v) + 4} fill="var(--muted)" fontSize="11">
                {v}%
              </text>
            </g>
          ))}
          {[0, 0.5, 1].map((f) => (
            <text key={f} x={pad.l + f * (W - pad.l - pad.r)} y={H - 6} fill="var(--muted)" fontSize="11" textAnchor={f === 0 ? "start" : f === 1 ? "end" : "middle"}>
              {fmt(t0 + f * (t1 - t0))}
            </text>
          ))}
          {drawn.map((s, i) => (
            <path
              key={s.name}
              d={s.points.map(([t, v], j) => `${j ? "L" : "M"}${x(Date.parse(t)).toFixed(1)},${y(v).toFixed(1)}`).join("")}
              fill="none"
              stroke={LINE_COLOURS[i % LINE_COLOURS.length]}
              strokeWidth={i === 0 ? 2.2 : 1.8}
              strokeLinejoin="round"
            />
          ))}
          {at !== null && <line x1={x(at)} x2={x(at)} y1={pad.t} y2={H - pad.b} stroke="var(--line-2)" />}
          {drawn.map((s, i) => {
            const last = s.points[s.points.length - 1]!;
            const t = at ?? Date.parse(last[0]);
            return <circle key={s.name} cx={x(t)} cy={y(at === null ? last[1] : valueAt(s, t))} r="3.5" fill={LINE_COLOURS[i % LINE_COLOURS.length]} />;
          })}
        </svg>
      </div>
      <div className="range" role="group" aria-label="Time range">
        {at !== null && <span className="muted readout">{fmt(at)} UTC</span>}
        {RANGES.map(([name]) => (
          <button key={name} type="button" aria-pressed={range === name} onClick={() => setRange(name)}>
            {name}
          </button>
        ))}
      </div>
    </div>
  );
}
