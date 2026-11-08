"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";

export function SignInForm() {
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [totp, setTotp] = useState("");
  const [needsCode, setNeedsCode] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const res = await fetch("/api/session", {
      method: "POST",
      headers: { "content-type": "application/json", "x-brier-request": "1" },
      body: JSON.stringify(needsCode ? { username, password, totp } : { username, password }),
    });
    if (res.ok) {
      router.push("/");
      router.refresh();
      return;
    }
    const body = (await res.json()) as { error?: { code?: string; message?: string } };
    if (body.error?.code === "totp_required") {
      setNeedsCode(true);
      setError("Enter the 6-digit code from your authenticator app.");
    } else {
      setError(body.error?.message ?? "Could not sign in.");
    }
  }

  return (
    <form className="card auth" onSubmit={submit}>
      <h1 className="display">Log in</h1>
      <label className="field">
        <span>Username</span>
        <input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
      </label>
      <label className="field">
        <span>Password</span>
        <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      {needsCode && (
        <label className="field">
          <span>Authenticator code</span>
          <input inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" value={totp} onChange={(e) => setTotp(e.target.value)} required autoFocus />
        </label>
      )}
      <button className="btn primary block" type="submit">
        Log in
      </button>
      <p className="muted" style={{ margin: "-6px 0 0", fontSize: 14, textAlign: "right" }}>
        <a className="link" href="/recover">
          Forgot your password?
        </a>
      </p>
      <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>
        New here? <a className="link" href="/register">Create an account</a> and get 10,000 play credits.
      </p>
    </form>
  );
}
