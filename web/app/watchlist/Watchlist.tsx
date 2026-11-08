"use client";
import { useEffect, useState } from "react";
import type { EventSummary } from "@/lib/events";
import { EventCard } from "@/components/EventCard";
import { useWatchlist } from "@/components/WatchButton";

/** Starred events, kept in this browser; prices are read fresh from the exchange. */
export function Watchlist({ now }: { now: number }) {
  const slugs = useWatchlist();
  const [events, setEvents] = useState<EventSummary[] | null>(null);
  useEffect(() => {
    let live = true;
    fetch("/api/v1/events?status=all&limit=200", { cache: "no-store" })
      .then((r) => (r.ok ? (r.json() as Promise<{ events: EventSummary[] }>) : { events: [] }))
      .then((b) => live && setEvents(b.events))
      .catch(() => live && setEvents([]));
    return () => {
      live = false;
    };
  }, []);
  if (events === null) return <p className="card empty">Loading your watchlist…</p>;
  const shown = events.filter((e) => slugs.includes(e.slug));
  if (shown.length === 0) return <p className="card empty">Star a market (☆ on any card) to keep it here. The list lives in this browser.</p>;
  return (
    <div className="grid-3">
      {shown.map((e) => (
        <EventCard key={e.slug} e={e} now={now} />
      ))}
    </div>
  );
}
