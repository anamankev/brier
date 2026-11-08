"use client";
import { useSyncExternalStore } from "react";
import { Star } from "./Icons";

// The watchlist lives in this browser only (localStorage): a convenience, not account data.
const KEY = "brier.watchlist";
const listeners = new Set<() => void>();

function read(): string[] {
  try {
    const raw = window.localStorage.getItem(KEY);
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
    window.localStorage.setItem(KEY, JSON.stringify(next));
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
