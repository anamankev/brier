// The social layer's shapes (G10 U3): profiles, holders, activity, notifications.

export type ProfileTrade = {
  seq: number;
  market: number;
  side: "buy" | "sell";
  price: number;
  qty: number;
  at: string | null;
  event: string | null;
  outcome: string | null;
  title: string | null;
};

export type Profile = {
  username: string;
  joined: string | null;
  role: string;
  settled_markets: number;
  profit: number;
  markets_traded: number;
  volume: number;
  scored_markets: number;
  average_skill: number | null;
  positions: { market: number; position: number; cash: number; event: string | null; outcome: string | null; title: string | null }[];
  trades: ProfileTrade[];
};

export type Holders = { yes: { username: string; contracts: number }[]; no: { username: string; contracts: number }[] };
export type Activity = { trades: { seq: number; username: string; side: "buy" | "sell"; price: number; qty: number; at: string | null }[] };

export type Notification = {
  seq: number;
  kind: "fill" | "settled" | "allowance";
  market: number | null;
  side: "buy" | "sell" | null;
  price: number | null;
  qty: number | null;
  result: "yes" | "no" | "void" | null;
  at: string | null;
  event: string | null;
  outcome: string | null;
  title: string | null;
  unread: boolean;
};
export type Notifications = { unread: number; items: Notification[] };

/** Where a market lives: its event page with the outcome selected, else its market page. */
export function marketHref(x: { market: number | null; event: string | null }): string {
  if (x.market == null) return "/portfolio";
  return x.event ? `/e/${x.event}?m=${x.market}` : `/markets/${x.market}`;
}

/** A trade in plain words, from the trader's side of a single YES book: buying YES at p is
 * "Yes at p¢"; selling YES at p is the same as buying NO at 100 − p. */
export function tradeWords(side: "buy" | "sell", price: number): { word: "Yes" | "No"; price: number } {
  return side === "buy" ? { word: "Yes", price } : { word: "No", price: 100 - price };
}

/** A user's display name: deleted accounts read as such. */
export function displayName(username: string): string {
  return username.startsWith("deleted_") ? "Deleted account" : username;
}
