"use client";
// The public market-data stream: the `book:<id>` channel (a snapshot, then deltas, each with
// the channel's sequence number) and `trades:<id>`. Same rules as the Python reference client:
// a delta that skips a sequence number means the book is incomplete, so resubscribe and wait
// for a fresh snapshot; never apply a gap.
import { useEffect, useRef, useState } from "react";


export type Level = [price: number, qty: number];
export type LiveTrade = { price: number; qty: number; taker_side: "buy" | "sell"; ts: number; seq: number };
export type Book = { synced: boolean; state: string; bids: Map<number, number>; asks: Map<number, number>; gaps: number };

type BookMessage = { type: "snapshot" | "delta"; seq: number; state: string; bids: Level[]; asks: Level[] };

/** Apply one book message; returns the new book, or "resubscribe" on a gap. */
export function applyBook(book: Book & { seq: number | null }, m: BookMessage): (Book & { seq: number | null }) | "resubscribe" | "ignored" {
  if (m.type === "snapshot") {
    if (book.seq !== null && m.seq <= book.seq) return "ignored";
    book = { ...book, bids: new Map(), asks: new Map() };
  } else {
    if (book.seq === null || m.seq <= book.seq) return "ignored";
    if (m.seq !== book.seq + 1) return "resubscribe";
    book = { ...book, bids: new Map(book.bids), asks: new Map(book.asks) };
  }
  for (const [side, levels] of [
    [book.bids, m.bids],
    [book.asks, m.asks],
  ] as const) {
    for (const [price, qty] of levels) {
      if (qty === 0) side.delete(price);
      else side.set(price, qty);
    }
  }
  return { ...book, seq: m.seq, state: m.state, synced: true };
}

export function useMarketData(market: number, wsUrl: string): { book: Book; trades: LiveTrade[]; connected: boolean } {
  const [book, setBook] = useState<Book & { seq: number | null }>({ synced: false, state: "", bids: new Map(), asks: new Map(), gaps: 0, seq: null });
  const [trades, setTrades] = useState<LiveTrade[]>([]);
  const [connected, setConnected] = useState(false);
  // The latest book, for the socket handlers (which update it before rendering it).
  const ref = useRef(book);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let closed = false;
    let retry = 500;
    const channel = `book:${market}`;
    const send = (op: string, ch: string) => socket?.send(JSON.stringify({ op, channel: ch }));
    const connect = () => {
      socket = new WebSocket(wsUrl);
      socket.onopen = () => {
        retry = 500;
        setConnected(true);
        send("subscribe", channel);
        send("subscribe", `trades:${market}`);
      };
      socket.onmessage = (event) => {
        const m = JSON.parse(String(event.data)) as { type: string; channel?: string } & Record<string, unknown>;
        if (m.channel === channel && (m.type === "snapshot" || m.type === "delta")) {
          const next = applyBook(ref.current, m as unknown as BookMessage);
          if (next === "resubscribe") {
            const stale = { ...ref.current, synced: false, seq: null, gaps: ref.current.gaps + 1 };
            ref.current = stale;
            setBook(stale);
            send("unsubscribe", channel);
            send("subscribe", channel);
          } else if (next !== "ignored") {
            ref.current = next;
            setBook(next);
          }
        } else if (m.type === "trade") {
          const t = m as unknown as LiveTrade;
          setTrades((old) => [t, ...old].slice(0, 200));
        }
      };
      socket.onclose = () => {
        setConnected(false);
        if (!closed) {
          setTimeout(connect, retry);
          retry = Math.min(retry * 2, 10_000);
        }
        const reset = { ...ref.current, synced: false, seq: null };
        ref.current = reset;
        setBook(reset);
      };
    };
    connect();
    return () => {
      closed = true;
      socket?.close();
    };
  }, [market, wsUrl]);

  return { book, trades, connected };
}
