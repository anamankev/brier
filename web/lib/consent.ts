"use client";
// Cookie and storage consent (the cookie banner). Brier has two kinds of storage: essential
// (the session cookie, this consent record) and preferences (the watchlist kept on this device).
// It has no analytics or advertising, so there is nothing else to consent to.
import { useSyncExternalStore } from "react";

export const CONSENT_COOKIE = "brier_consent";
const MAX_AGE = 180 * 24 * 3600; // ask again after six months
export const OPEN_SETTINGS = "brier:cookie-settings";

export type Consent = { preferences: boolean };
const listeners = new Set<() => void>();

function readRaw(): string {
  if (typeof document === "undefined") return "";
  const hit = document.cookie.split("; ").find((c) => c.startsWith(`${CONSENT_COOKIE}=`));
  return hit ? decodeURIComponent(hit.slice(CONSENT_COOKIE.length + 1)) : "";
}

/** The visitor's choice, or null before they have made one. */
export function readConsent(): Consent | null {
  const raw = readRaw();
  if (raw === "v1.p1") return { preferences: true };
  if (raw === "v1.p0") return { preferences: false };
  return null;
}

export function saveConsent(c: Consent) {
  const secure = location.protocol === "https:" ? "; Secure" : "";
  document.cookie = `${CONSENT_COOKIE}=v1.p${c.preferences ? 1 : 0}; Max-Age=${MAX_AGE}; Path=/; SameSite=Lax${secure}`;
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** The current choice as a string React can compare ("" before mount or before a choice). */
export function useConsentRaw(): string {
  return useSyncExternalStore(subscribe, readRaw, () => "");
}

export function openCookieSettings() {
  window.dispatchEvent(new Event(OPEN_SETTINGS));
}
