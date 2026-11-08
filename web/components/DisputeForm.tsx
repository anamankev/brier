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
      aria-label="Dispute this resolution"
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          await api("POST", `/v1/resolutions/${resolution}/disputes`, { reason });
          setMessage({ ok: true, text: "Dispute filed. Settlement waits until an admin rules on it." });
          router.refresh();
        } catch (err) {
          setMessage({ ok: false, text: err instanceof ApiError ? err.message : "Could not file the dispute." });
        }
      }}
    >
      <label>
        Why is this outcome wrong? (3–500 characters)
        <textarea value={reason} onChange={(e) => setReason(e.target.value)} minLength={3} maxLength={500} required rows={3} />
      </label>
      <button type="submit">File a dispute</button>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
