"use client";
import { useSyncExternalStore } from "react";
import { readConsent } from "@/lib/consent";
import { Star } from "./Icons";

// The watchlist lives in this browser only, never on the server. With the "preferences" cookie
// choice it is kept on this device (localStorage); without it, only for this browsing session
// (sessionStorage), which starring a market is an explicit request for.
const KEY = "brier.watchlist";
const listeners = new Set<() => void>();

function store(): Storage {
  return readConsent()?.preferences ? window.localStorage : window.sessionStorage;
}

/** Move the list to where the current choice keeps it (called when the choice changes). */
export function rehomeWatchlist() {
  try {
    const list = read();
    window.localStorage.removeItem(KEY);
    window.sessionStorage.removeItem(KEY);
    if (list.length) store().setItem(KEY, JSON.stringify(list));
  } catch {
    // storage blocked
  }
  listeners.forEach((l) => l());
}

function read(): string[] {
  try {
    const raw = window.localStorage.getItem(KEY) ?? window.sessionStorage.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((s): s is string => typeof s === "string") : [];
  } catch {
    return [];
  }
}

let cached = "";
function snapshot(): string {
  const next = JSON.stringify(read());
  if (next !== cached) cached = next;
  return cached;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  window.addEventListener("storage", listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", listener);
  };
}

export function useWatchlist(): string[] {
  const raw = useSyncExternalStore(subscribe, snapshot, () => "[]");
  return JSON.parse(raw) as string[];
}

export function toggleWatch(slug: string) {
  const list = read();
  const next = list.includes(slug) ? list.filter((s) => s !== slug) : [...list, slug];
  try {
    store().setItem(KEY, JSON.stringify(next));
  } catch {
    // storage blocked: the star simply does not stick
  }
  listeners.forEach((l) => l());
}

export function WatchButton({ slug, title }: { slug: string; title: string }) {
  const watched = useWatchlist().includes(slug);
  return (
    <button
      type="button"
      className={`icon-btn${watched ? " on" : ""}`}
      aria-pressed={watched}
      aria-label={watched ? `Remove “${title}” from your watchlist` : `Add “${title}” to your watchlist`}
      onClick={() => toggleWatch(slug)}
    >
      <Star filled={watched} />
    </button>
  );
}
