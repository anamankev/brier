// Display helpers. Prices are integer ticks 1-99: cents per YES contract, and the market's
// probability in percent. Money is integer credits (cents).
export function credits(cents: number): string {
  const sign = cents < 0 ? "−" : "";
  const abs = Math.abs(cents);
  return `${sign}${Math.floor(abs / 100).toLocaleString("en-US")}.${String(abs % 100).padStart(2, "0")}`;
}

export function percent(ticks: number | null | undefined): string {
  return ticks == null ? "—" : `${ticks}%`;
}

export function when(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return d.toLocaleString("en-US", { timeZone: "UTC", dateStyle: "medium", timeStyle: "short" }) + " UTC";
}

/** Skill and Brier scores are integers in units of 1/10,000. */
export function score(units: number | null | undefined): string {
  return units == null ? "—" : (units / 10_000).toFixed(4);
}

export const STATE_LABEL: Record<string, string> = {
  pre_open: "Not open yet",
  open: "Open",
  halted: "Halted",
  closed: "Closed",
  resolved: "Resolved",
  voided: "Voided",
};

/** "62%" or "—". */
export function chance(p: number | null | undefined): string {
  return p == null ? "—" : `${p}%`;
}

/** A price in cents: "62¢". */
export function cents(p: number | null | undefined): string {
  return p == null ? "—" : `${p}¢`;
}

/** What a winning contract returns per unit staked at this price: "1.6x". */
export function multiple(price: number | null | undefined): string {
  return price == null || price <= 0 ? "" : `${(100 / price).toFixed(2)}x`;
}

/** "Dec 10" in UTC. */
export function shortDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
}

/** "in 32 days", "in 5 hours", "3 days ago", relative to `now` (milliseconds). */
export function relative(iso: string, now: number): string {
  const ms = Date.parse(iso) - now;
  const abs = Math.abs(ms);
  const [n, unit] =
    abs >= 86_400_000 ? [Math.round(abs / 86_400_000), "day"] : abs >= 3_600_000 ? [Math.round(abs / 3_600_000), "hour"] : [Math.max(1, Math.round(abs / 60_000)), "minute"];
  const words = `${n} ${unit}${n === 1 ? "" : "s"}`;
  return ms >= 0 ? `in ${words}` : `${words} ago`;
}
