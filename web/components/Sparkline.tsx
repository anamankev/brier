/** A tiny price line (0-100%), coloured by direction, with a soft fill. Decorative: the number
 * beside it says the same thing in text. */
export function Sparkline({ points, width = 160, height = 44 }: { points: [string, number][]; width?: number; height?: number }) {
  if (points.length < 2) {
    return <svg width={width} height={height} aria-hidden="true"><line x1="0" x2={width} y1={height - 2} y2={height - 2} stroke="var(--line)" strokeDasharray="2 4" /></svg>;
  }
  const values = points.map((p) => p[1]);
  const lo = Math.max(0, Math.min(...values) - 5);
  const hi = Math.min(100, Math.max(...values) + 5);
  const x = (i: number) => (i / (points.length - 1)) * width;
  const y = (v: number) => height - 3 - ((v - lo) / Math.max(1, hi - lo)) * (height - 6);
  const d = values.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const up = values[values.length - 1]! >= values[0]!;
  const colour = up ? "var(--yes)" : "var(--no)";
  const id = `spark-${up ? "u" : "d"}`;
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      <defs>
        <linearGradient id={id} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor={colour} stopOpacity="0.28" />
          <stop offset="1" stopColor={colour} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={`${d}L${width},${height}L0,${height}Z`} fill={`url(#${id})`} />
      <path d={d} fill="none" stroke={colour} strokeWidth="1.8" strokeLinejoin="round" />
    </svg>
  );
}
