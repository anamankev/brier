"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";

export default function SignIn() {
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
    <>
      <h1>Sign in</h1>
      <form className="stack" onSubmit={submit}>
        <label>
          Username
          <input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
        </label>
        <label>
          Password
          <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
        </label>
        {needsCode && (
          <label>
            Authenticator code
            <input inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" value={totp} onChange={(e) => setTotp(e.target.value)} required autoFocus />
          </label>
        )}
        <button className="primary" type="submit">
          Sign in
        </button>
        <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
      </form>
    </>
  );
}
