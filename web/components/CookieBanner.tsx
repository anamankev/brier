"use client";
import Link from "next/link";
import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { OPEN_SETTINGS, readConsent, saveConsent, useConsentRaw } from "@/lib/consent";
import { rehomeWatchlist } from "./WatchButton";

const noop = () => () => {};

/**
 * The cookie banner: shown until the visitor chooses, and again from "Cookie settings" in the
 * footer. Essential storage is always on; preferences (the watchlist kept on this device) is a
 * real choice. There are no analytics or advertising cookies to offer.
 */
export function CookieBanner() {
  const chosen = useConsentRaw();
  // False while rendering on the server: the choice lives in a cookie the browser reads.
  const mounted = useSyncExternalStore(noop, () => true, () => false);
  const [custom, setCustom] = useState(false);
  const [reopened, setReopened] = useState(false);
  const [prefs, setPrefs] = useState(true);
  const title = useId();
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const open = () => {
      setPrefs(readConsent()?.preferences ?? true);
      setCustom(true);
      setReopened(true);
    };
    window.addEventListener(OPEN_SETTINGS, open);
    return () => window.removeEventListener(OPEN_SETTINGS, open);
  }, []);
  useEffect(() => {
    if (reopened) box.current?.querySelector<HTMLElement>("input, button")?.focus();
  }, [reopened]);

  if (!mounted || (chosen && !reopened)) return null;

  const choose = (preferences: boolean) => {
    saveConsent({ preferences });
    rehomeWatchlist();
    setReopened(false);
    setCustom(false);
  };

  return (
    <div ref={box} className="cookie-banner card" role="dialog" aria-modal="false" aria-labelledby={title}>
      <h2 id={title}>Cookies and your privacy</h2>
      {!custom ? (
        <p>
          Brier uses one essential cookie to keep you signed in. With your permission it also remembers your watchlist on
          this device. No ads, no analytics, no tracking. <Link href="/cookies">Cookie policy</Link>
        </p>
      ) : (
        <ul className="cookie-choices">
          <li>
            <div>
              <b>Essential</b>
              <span>Your sign-in session and this choice. Always on: the site cannot work without them.</span>
            </div>
            <label className="switch">
              <input type="checkbox" checked disabled aria-label="Essential cookies (always on)" />
              <span aria-hidden="true" />
            </label>
          </li>
          <li>
            <div>
              <b>Preferences</b>
              <span>Keep your watchlist on this device between visits. Off: it lasts until you close the browser.</span>
            </div>
            <label className="switch">
              <input type="checkbox" checked={prefs} onChange={(e) => setPrefs(e.target.checked)} aria-label="Preference storage" />
              <span aria-hidden="true" />
            </label>
          </li>
          <li className="none">
            <div>
              <b>Analytics and advertising</b>
              <span>Brier does not use any.</span>
            </div>
          </li>
        </ul>
      )}
      <div className="cookie-actions">
        {custom ? (
          <>
            <button type="button" className="btn primary" onClick={() => choose(prefs)}>
              Save choices
            </button>
            {reopened && chosen && (
              <button type="button" className="btn" onClick={() => (setReopened(false), setCustom(false))}>
                Cancel
              </button>
            )}
          </>
        ) : (
          <>
            <button type="button" className="btn primary" onClick={() => choose(true)}>
              Accept all
            </button>
            <button type="button" className="btn" onClick={() => choose(false)}>
              Essential only
            </button>
            <button type="button" className="btn ghost" onClick={() => (setPrefs(true), setCustom(true))}>
              Customize
            </button>
          </>
        )}
      </div>
    </div>
  );
}

/** "Cookie settings" in the footer: reopens the banner's choices. */
export function CookieSettingsLink() {
  return (
    <button type="button" className="link-button" onClick={() => window.dispatchEvent(new Event(OPEN_SETTINGS))}>
      Cookie settings
    </button>
  );
}
