"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";
import { RecoveryCodes } from "@/components/RecoveryCodes";

export function NewCodes({ username }: { username: string }) {
  const [codes, setCodes] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  async function make() {
    setError(null);
    try {
      const r = await api<{ recovery_codes: string[] }>("POST", "/v1/recovery-codes");
      setCodes(r.recovery_codes);
      setConfirming(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not make new codes.");
    }
  }

  if (codes) return <RecoveryCodes codes={codes} username={username} doneLabel="Done" onDone={() => setCodes(null)} />;
  return (
    <div className="stack" style={{ gap: 10 }}>
      {confirming ? (
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn primary" onClick={make}>
            Replace my codes
          </button>
          <button type="button" className="btn" onClick={() => setConfirming(false)}>
            Keep the old ones
          </button>
        </div>
      ) : (
        <button type="button" className="btn" style={{ justifySelf: "start" }} onClick={() => setConfirming(true)}>
          Make new recovery codes
        </button>
      )}
      {confirming && <p className="muted" style={{ margin: 0, fontSize: 14 }}>Your old codes stop working as soon as the new ones are made.</p>}
      <div aria-live="polite">{error && <p className="notice error">{error}</p>}</div>
    </div>
  );
}

export function ExportData({ username }: { username: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function download() {
    setBusy(true);
    setError(null);
    try {
      const data = await api<unknown>("GET", "/v1/me/export");
      const url = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: "application/json" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = `brier-${username}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not export your data.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="stack" style={{ gap: 10 }}>
      <button type="button" className="btn" style={{ justifySelf: "start" }} onClick={download} disabled={busy}>
        {busy ? "Preparing…" : "Download my data"}
      </button>
      <div aria-live="polite">{error && <p className="notice error">{error}</p>}</div>
    </div>
  );
}

export function DeleteAccount({ username }: { username: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function remove(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api("DELETE", "/v1/me", { password });
      await fetch("/api/session", { method: "DELETE", headers: { "x-brier-request": "1" } });
      router.push("/");
      router.refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not delete the account.");
      setBusy(false);
    }
  }

  if (!open) {
    return (
      <button type="button" className="btn danger" style={{ justifySelf: "start" }} onClick={() => setOpen(true)}>
        Delete my account
      </button>
    );
  }
  return (
    <form className="stack" style={{ gap: 12 }} onSubmit={remove}>
      <p className="dim" style={{ margin: 0 }}>
        Your open orders are cancelled and you can never log in again. Your username, password and sign-in history are
        erased. Trades you made stay in the exchange&apos;s ledger under an anonymous name, because every trade has another
        side. This cannot be undone.
      </p>
      <label className="field">
        <span>Your password</span>
        <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      <label className="field">
        <span>
          Type <b>{username}</b> to confirm
        </span>
        <input autoComplete="off" value={typed} onChange={(e) => setTyped(e.target.value)} required />
      </label>
      <div className="row" style={{ gap: 8 }}>
        <button type="submit" className="btn danger" disabled={busy || typed !== username || !password}>
          {busy ? "Deleting…" : "Delete forever"}
        </button>
        <button type="button" className="btn" onClick={() => setOpen(false)}>
          Cancel
        </button>
      </div>
      <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
    </form>
  );
}
