"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";

export function CloseCase({ id }: { id: number }) {
  const router = useRouter();
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const close = async (outcome: "dismissed" | "actioned") => {
    setBusy(true);
    setError(null);
    try {
      await api("POST", `/v1/admin/cases/${id}/close`, { outcome, reason });
      router.refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "the case could not be closed");
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="card pad" aria-labelledby="close-h">
      <h2 id="close-h" style={{ fontSize: 18, marginBottom: 10 }}>Close this case</h2>
      <label htmlFor="close-reason" className="field">
        <span>Reason (3–500 characters, written to the audit log)</span>
        <input id="close-reason" value={reason} onChange={(e) => setReason(e.target.value)} minLength={3} maxLength={500} required />
      </label>
      <div className="row" style={{ marginTop: 12 }}>
        <button type="button" className="btn" disabled={busy || reason.trim().length < 3} onClick={() => close("dismissed")}>Dismiss</button>
        <button type="button" className="btn danger" disabled={busy || reason.trim().length < 3} onClick={() => close("actioned")}>Mark actioned</button>
      </div>
      <div aria-live="polite">{error && <p className="notice error" style={{ marginTop: 12 }}>{error}</p>}</div>
    </section>
  );
}
