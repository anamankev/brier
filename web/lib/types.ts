export type MarketState = "pre_open" | "open" | "halted" | "closed" | "resolved" | "voided";

export type MarketRow = {
  market: number;
  state: MarketState;
  title?: string;
  category?: string;
  close?: string;
  last_price?: number | null;
  volume?: number;
};

export type Rules = {
  market: number;
  rules_hash: string;
  rulebook: Record<string, unknown> & { title: string; question: string; source: { provider: string; series: string; url: string } };
  canonical: string;
  text: string;
  schedule: { open: string; close: string; release: string; deadline: string };
  journal: { listed_seq: number; rules_hash: string | null; matches: boolean } | null;
};

export type Resolution = {
  resolution: number;
  outcome: "yes" | "no" | "void";
  void_condition: string | null;
  evidence_hash: string;
  state: "proposed" | "confirmed" | "settling" | "settled" | "withdrawn";
  proposed_at: string;
  confirmed_at: string | null;
  dispute_until: string | null;
  settled_seq: number | null;
  evidence: Record<string, unknown>;
  disputes: { open: number; total: number };
};

export type ResolutionView = {
  market: number;
  resolutions: Resolution[];
  journal: { settled_seq: number; outcome: string; evidence_hash: string | null } | null;
};

export type Portfolio = {
  as_of: number;
  cash: number;
  reserved: number;
  available: number;
  positions: { market: number; position: number }[];
  open_orders: { order: number; market: number; side: "buy" | "sell"; price: number; remaining: number }[];
};
