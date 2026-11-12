"use client";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState } from "react";
import { api } from "@/lib/client";
import { TERMS_DATE, TERMS_VERSION } from "@/lib/terms";

/** Shown to a signed-in user who accepted an earlier version of the terms: orders are refused
 * until they accept the current one. */
export function TermsUpdate() {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function accept() {
    setBusy(true);
    setError("");
    try {
      await api("POST", "/v1/terms", { terms: TERMS_VERSION });
      router.refresh();
    } catch {
      setError("That did not work. Please try again.");
      setBusy(false);
    }
  }
  return (
    <div className="terms-update" role="region" aria-label="Updated terms">
      <span>
        Our <Link href="/terms">terms of service</Link> and <Link href="/privacy">privacy policy</Link> changed on {TERMS_DATE}.
        Accept the new terms to keep trading.
      </span>
      {error && <span className="no">{error}</span>}
      <button type="button" className="btn primary" onClick={accept} disabled={busy}>
        Accept new terms
      </button>
    </div>
  );
}
