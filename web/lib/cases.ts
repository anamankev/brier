// Surveillance cases (docs/surveillance.md), as the gateway returns them.
export type Typology = "wash" | "spoof" | "close" | "insider";

export type Case = {
  case: number;
  typology: Typology;
  market: number;
  market_title: string | null;
  account: number;
  username: string | null;
  score: number;
  detector: string;
  status: "open" | "dismissed" | "actioned";
  opened_at: string;
  closed_at: string | null;
  closed_by: string | null;
  close_reason: string | null;
};

export type CaseDetail = Case & {
  evidence: Record<string, unknown> & {
    timeline?: unknown[][];
    pnl?: { cash: number; position: number; value: number; valued_at: string };
  };
  linked_accounts: { account: number; username: string }[];
};

export const TYPOLOGY: Record<Typology, { name: string; what: string }> = {
  wash: {
    name: "Wash trading",
    what: "Trades between accounts that share a login address, as a share of this account's volume in the market.",
  },
  spoof: {
    name: "Spoofing",
    what: "Large orders near the best price, cancelled unfilled soon after the account traded the other way.",
  },
  close: {
    name: "Marking the close",
    what: "A large share of aggressive volume just before the close, with the last price moved in its direction.",
  },
  insider: {
    name: "Insider-like trading",
    what: "A position built just before the close on the side that won, out of line with the account's earlier activity.",
  },
};

/** Sequence numbers the detector named as the evidence, to highlight on the timeline. */
export function evidenceSeqs(evidence: Record<string, unknown>): Set<number> {
  const out = new Set<number>();
  const nums = (v: unknown) => (Array.isArray(v) ? v.filter((x): x is number => typeof x === "number") : []);
  nums(evidence.trade_seqs).forEach((s) => out.add(s));
  if (Array.isArray(evidence.orders)) {
    for (const o of evidence.orders as Record<string, unknown>[]) {
      if (typeof o.order_seq === "number") out.add(o.order_seq);
      nums(o.against_seqs).forEach((s) => out.add(s));
    }
  }
  return out;
}

/** A score in its typology's own terms: wash trading is a share of volume. */
export function scoreText(c: { typology: Typology; score: number }): string {
  return c.typology === "wash" ? `${Math.round(c.score * 100)}% of volume` : c.score.toFixed(2);
}
