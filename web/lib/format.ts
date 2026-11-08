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
