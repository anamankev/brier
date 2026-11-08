"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";

export function DisputeForm({ resolution }: { resolution: number }) {
  const router = useRouter();
  const [reason, setReason] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  return (
    <form
      className="stack"
      style={{ marginTop: 18 }}
      aria-label="Dispute this outcome"
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          await api("POST", `/v1/resolutions/${resolution}/disputes`, { reason });
          setMessage({ ok: true, text: "Dispute filed. Payout waits until an admin rules on it." });
          router.refresh();
        } catch (err) {
          setMessage({ ok: false, text: err instanceof ApiError ? err.message : "Could not file the dispute." });
        }
      }}
    >
      <label className="field">
        <span>Think this outcome is wrong? Tell us why (3–500 characters)</span>
        <textarea value={reason} onChange={(e) => setReason(e.target.value)} minLength={3} maxLength={500} required rows={3} style={{ fontFamily: "inherit", fontSize: 14 }} />
      </label>
      <div>
        <button type="submit" className="btn danger">
          File a dispute
        </button>
      </div>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
