// The rules in plain words for the event page. The exact rulebook, its hash and the evidence stay
// one click away (/markets/{id}); this is what a person reads before trading.

type Rulebook = {
  source?: { provider?: string; series?: string; game?: { starts?: string } };
  condition?: { op?: string; threshold?: string; upper?: string };
  observation?: string;
  schedule?: { deadline?: string };
  event?: { slug?: string; title?: string; outcome?: string };
};

const longDate = (iso: string) =>
  new Date(iso.length === 10 ? `${iso}T12:00:00Z` : iso).toLocaleDateString("en-US", { month: "long", day: "numeric", timeZone: "UTC" });

const VOID = (deadline?: string) =>
  deadline ? ` If no official result is published by ${longDate(deadline)}, the market is voided and every contract pays 50¢.` : "";

/** "53-54°F" → "between 53°F and 54°F"; "48°F or below" stays. */
function range(label: string): string {
  const m = /^(-?\d+)-(-?\d+)(°F|%)$/.exec(label.trim());
  return m ? `between ${m[1]}${m[3]} and ${m[2]}${m[3]}` : label;
}

/** Plain-language rules for one outcome, or the gateway's own text with readable dates. */
export function plainRules(rb: Rulebook, fallback: string): string {
  const provider = rb.source?.provider ?? "";
  const outcome = rb.event?.outcome ?? "";
  const deadline = rb.schedule?.deadline;
  if ((provider === "balldontlie" || provider === "football_data") && outcome) {
    const soccer = provider === "football_data";
    const when = rb.source?.game?.starts ? ` on ${longDate(rb.source.game.starts)}` : "";
    const scope = soccer ? "at full time (90 minutes plus stoppage time; extra time and penalties do not count)" : "in the final score, counting overtime";
    const what = rb.condition?.op === "==" ? `the game${when} ends in a draw ${scope}` : `the ${outcome} win the game${when} ${scope}`;
    return `Resolves Yes if ${what}. Resolves No otherwise. Trading stops at kickoff.${VOID(deadline)}`;
  }
  if (provider === "nws" && outcome && rb.observation) {
    const low = (rb.source?.series ?? "").endsWith("/MIN");
    const city = /in (.+?) on /.exec(rb.event?.title ?? "")?.[1] ?? "the city";
    return `Resolves Yes if the official ${low ? "low" : "high"} temperature in ${city} on ${longDate(rb.observation)} is ${range(outcome)}, as first reported in the National Weather Service's daily climate report the next morning. Resolves No if it is reported and is not.${VOID(deadline)}`;
  }
  return fallback
    .replace(/(\d{4}-\d{2}-\d{2})T(\d{2}):(\d{2}):\d{2}Z/g, (_, d) => longDate(d))
    .replace(/for the day (\d{4}-\d{2}-\d{2})/g, (_, d) => `on ${longDate(d)}`)
    .replace(/the value of /g, "")
    .replace(/Resolves YES/g, "Resolves Yes")
    .replace(/Resolves NO/g, "Resolves No");
}
