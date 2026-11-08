"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";

/** The daily allowance: a button while today's is unclaimed, gone once claimed. */
export function DailyCredits({ amount }: { amount: number }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function claim() {
    setBusy(true);
    setError(null);
    try {
      await api("POST", "/v1/allowance");
      setDone(true);
      router.refresh();
    } catch (err) {
      if (err instanceof ApiError && err.status === 409) setDone(true);
      else setError("Could not claim; try again.");
    } finally {
      setBusy(false);
    }
  }

  if (done) return null;
  return (
    <button type="button" className="daily" onClick={claim} disabled={busy} title={error ?? `Claim your free ${amount / 100} credits for today`}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" aria-hidden="true">
        <path d="M4 11h16v9H4zM2.5 7h19v4h-19zM12 7v13" strokeLinejoin="round" />
        <path d="M12 7c-1.5-3-5-3.5-5-1.2C7 7 9 7 12 7Zm0 0c1.5-3 5-3.5 5-1.2C17 7 15 7 12 7Z" strokeLinejoin="round" />
      </svg>
      {busy ? "Claiming…" : `+${amount / 100} daily`}
    </button>
  );
}
