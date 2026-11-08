// Server-side reads of events from the gateway (the shared types and helpers are in ./events).
import { gateway } from "./server";
import type { EventList, EventQuery, EventSummary } from "./events";

export async function listEvents(query: EventQuery = {}): Promise<EventList | null> {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) if (v !== undefined && v !== "") params.set(k, String(v));
  const res = await gateway<EventList>(`/v1/events?${params}`);
  return res.ok ? res.body : null;
}

export async function getEvent(slug: string): Promise<EventSummary | null> {
  const res = await gateway<EventSummary>(`/v1/events/${encodeURIComponent(slug)}`);
  return res.ok ? res.body : null;
}
