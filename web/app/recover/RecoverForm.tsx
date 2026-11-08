"use client";
import Link from "next/link";
import { useState } from "react";
import { api, ApiError } from "@/lib/client";

export function RecoverForm() {
  const [username, setUsername] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [totp, setTotp] = useState("");
  const [needsTotp, setNeedsTotp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [left, setLeft] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const body = { username, code, new_password: password, ...(needsTotp ? { totp } : {}) };
      const r = await api<{ codes_left: number }>("POST", "/v1/recover", body);
      setLeft(r.codes_left);
    } catch (err) {
      if (err instanceof ApiError && err.code === "totp_required") {
        setNeedsTotp(true);
        setError("This account also needs the 6-digit code from your authenticator app.");
      } else if (err instanceof ApiError && err.code === "rate_limited") {
        setError("Too many attempts. Wait a minute and try again.");
      } else {
        setError(err instanceof ApiError ? err.message : "Could not reset the password.");
      }
    } finally {
      setBusy(false);
    }
  }

  if (left != null) {
    return (
      <div className="card auth">
        <h1 className="display">Password changed</h1>
        <p className="dim" style={{ margin: 0 }}>
          You were signed out everywhere. You have {left} recovery {left === 1 ? "code" : "codes"} left
          {left <= 3 ? ": make new ones from your account page after you log in." : "."}
        </p>
        <Link className="btn primary block" href="/signin">
          Log in
        </Link>
      </div>
    );
  }

  return (
    <form className="card auth" onSubmit={submit}>
      <h1 className="display">Reset your password</h1>
      <p className="muted" style={{ margin: 0 }}>
        Use one of the recovery codes you saved when you signed up. Each code works once.
      </p>
      <label className="field">
        <span>Username</span>
        <input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value.toLowerCase())} required />
      </label>
      <label className="field">
        <span>Recovery code</span>
        <input autoComplete="off" spellCheck={false} placeholder="XXXX-XXXX-XXXX-XXXX" value={code} onChange={(e) => setCode(e.target.value)} required />
      </label>
      <label className="field">
        <span>New password (at least 12 characters)</span>
        <input type="password" autoComplete="new-password" minLength={12} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      {needsTotp && (
        <label className="field">
          <span>Authenticator code</span>
          <input inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" value={totp} onChange={(e) => setTotp(e.target.value)} required autoFocus />
        </label>
      )}
      <button className="btn primary block" type="submit" disabled={busy}>
        {busy ? "Resetting…" : "Reset password"}
      </button>
      <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>
        Lost your codes too? Brier has no email, so the account cannot be recovered; you can{" "}
        <Link className="link" href="/register">
          create a new one
        </Link>
        .
      </p>
    </form>
  );
}
