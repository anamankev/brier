// Small inline icons. Decorative unless labelled.

/** Brier's mark: a calibration plot, forecasts against outcomes, with points on the diagonal. */
export function Mark({ size = 26 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <rect width="32" height="32" rx="8" fill="var(--brand)" />
      <path d="M8 24 L24 8" stroke="var(--brand-ink)" strokeWidth="2.4" strokeLinecap="round" opacity="0.35" />
      <circle cx="10.5" cy="21.5" r="2.4" fill="var(--brand-ink)" />
      <circle cx="16" cy="15.6" r="2.4" fill="var(--brand-ink)" />
      <circle cx="21.5" cy="10.5" r="2.4" fill="var(--brand-ink)" />
    </svg>
  );
}

export function SearchIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" strokeLinecap="round" />
    </svg>
  );
}

export function Check() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" aria-hidden="true">
      <path d="m5 12 5 5L20 7" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function Chevron() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" aria-hidden="true">
      <path d="m9 6 6 6-6 6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** The category tile: economics (index bars), rates (percent), weather (thermometer),
 * earthquakes (a seismogram), space weather (the sun behind a magnetic field line). */
export function CategoryTile({ category, large = false }: { category?: string | null; large?: boolean }) {
  const s = large ? 30 : 22;
  const common = { width: s, height: s, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "aria-hidden": true } as const;
  const glyph =
    category === "rates" ? (
      <svg {...common} strokeWidth="2.4">
        <path d="M19 5 5 19" strokeLinecap="round" />
        <circle cx="7" cy="7" r="2.6" />
        <circle cx="17" cy="17" r="2.6" />
      </svg>
    ) : category === "weather" ? (
      <svg {...common} strokeWidth="2.4">
        <path d="M10 4a2 2 0 1 1 4 0v9.3a4 4 0 1 1-4 0Z" />
        <path d="M12 9v6" strokeLinecap="round" />
      </svg>
    ) : category === "earthquakes" ? (
      <svg {...common} strokeWidth="2.2">
        <path d="M2 12h4l2-5 3 11 3-14 3 10 2-2h3" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    ) : category === "space" ? (
      <svg {...common} strokeWidth="2.2">
        <circle cx="12" cy="12" r="3.4" />
        <path d="M3.5 15.5c2.5-6.5 14.5-6.5 17 0M5 19c3-3.5 11-3.5 14 0" strokeLinecap="round" />
      </svg>
    ) : (
      <svg {...common} strokeWidth="2.6">
        <path d="M5 20V13M10 20V9M15 20v-5M20 20V5" strokeLinecap="round" />
      </svg>
    );
  return <span className={`tile ${category ?? "economics"}${large ? " lg" : ""}`}>{glyph}</span>;
}

export function Star({ filled = false }: { filled?: boolean }) {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d="m12 3.5 2.6 5.3 5.9.9-4.2 4.1 1 5.8-5.3-2.8-5.3 2.8 1-5.8-4.2-4.1 5.9-.9Z" strokeLinejoin="round" />
    </svg>
  );
}

export function Repeat() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" aria-hidden="true">
      <path d="M4 10V8a3 3 0 0 1 3-3h12l-3-3M20 14v2a3 3 0 0 1-3 3H5l3 3" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
