"use client";
import Link from "next/link";
import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";

/** The signed-in user's menu: profile, portfolio, account settings, and sign out. */
export function UserMenu({ username, admin }: { username: string; admin: boolean }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === "Escape" : !box.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);

  async function signOut() {
    await fetch("/api/session", { method: "DELETE", headers: { "x-brier-request": "1" } });
    setOpen(false);
    router.push("/");
    router.refresh();
  }

  const go = () => setOpen(false);
  return (
    <div className="usermenu" ref={box}>
      <button type="button" className="me-btn" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="avatar sm" aria-hidden="true">
          {username.slice(0, 1).toUpperCase()}
        </span>
        <span className="me-name">{username}</span>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" aria-hidden="true">
          <path d="m6 9 6 6 6-6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {open && (
        <div className="menu card" role="menu" aria-label="Account">
          <Link role="menuitem" href={`/u/${encodeURIComponent(username)}`} onClick={go}>
            Profile
          </Link>
          <Link role="menuitem" href="/portfolio" onClick={go}>
            Portfolio
          </Link>
          <Link role="menuitem" href="/scorecard" onClick={go}>
            Scorecard
          </Link>
          <Link role="menuitem" href="/watchlist" onClick={go}>
            Watchlist
          </Link>
          <Link role="menuitem" href="/account" onClick={go}>
            Account settings
          </Link>
          {admin && (
            <Link role="menuitem" href="/admin" onClick={go}>
              Admin
            </Link>
          )}
          <hr />
          <button type="button" role="menuitem" onClick={signOut}>
            Sign out
          </button>
        </div>
      )}
    </div>
  );
}
