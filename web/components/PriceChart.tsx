"use client";

/** Price over time as an SVG line (0-100%), with a text summary for screen readers. */
export function PriceChart({ points }: { points: [string, number][] }) {
  const W = 640;
  const H = 220;
  const pad = { l: 34, r: 8, t: 10, b: 22 };
  if (points.length < 2) {
    return <p className="muted">Not enough price history yet: the price is recorded once a minute while trading is open.</p>;
  }
  const t0 = Date.parse(points[0]![0]);
  const t1 = Date.parse(points[points.length - 1]![0]);
  const x = (t: string) => pad.l + ((Date.parse(t) - t0) / Math.max(1, t1 - t0)) * (W - pad.l - pad.r);
  const y = (p: number) => pad.t + (1 - p / 100) * (H - pad.t - pad.b);
  const d = points.map(([t, p], i) => `${i === 0 ? "M" : "L"}${x(t).toFixed(1)},${y(p).toFixed(1)}`).join(" ");
  const first = points[0]![1];
  const last = points[points.length - 1]![1];
  const lo = Math.min(...points.map((p) => p[1]));
  const hi = Math.max(...points.map((p) => p[1]));
  const summary = `Price moved from ${first}% to ${last}% over ${points.length} minute samples; low ${lo}%, high ${hi}%.`;
  return (
    <figure className="chart" style={{ margin: 0 }}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={summary}>
        <g className="grid">
          {[0, 25, 50, 75, 100].map((p) => (
            <g key={p}>
              <line x1={pad.l} x2={W - pad.r} y1={y(p)} y2={y(p)} />
              <text x={4} y={y(p) + 4}>
                {p}%
              </text>
            </g>
          ))}
        </g>
        <path className="line" d={d} />
      </svg>
      <figcaption className="muted" style={{ fontSize: "0.85rem" }}>
        {summary}
      </figcaption>
    </figure>
  );
}
