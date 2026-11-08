"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";

export default function Register() {
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      await api("POST", "/v1/register", { username, password });
      await fetch("/api/session", {
        method: "POST",
        headers: { "content-type": "application/json", "x-brier-request": "1" },
        body: JSON.stringify({ username, password }),
      });
      router.push("/account");
      router.refresh();
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Could not create the account.");
    }
  }

  return (
    <form className="card auth" onSubmit={submit}>
      <h1 className="display">Create your account</h1>
      <p className="muted" style={{ margin: 0 }}>
        Start with 10,000 play credits. They have no cash value and cannot be bought or withdrawn.
      </p>
      <label className="field">
        <span>Username (3–32 characters: a–z, 0–9, _)</span>
        <input autoComplete="username" pattern="[a-z0-9_]{3,32}" value={username} onChange={(e) => setUsername(e.target.value)} required />
      </label>
      <label className="field">
        <span>Password (at least 12 characters)</span>
        <input type="password" autoComplete="new-password" minLength={12} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      <button className="btn primary block" type="submit">
        Create account
      </button>
      <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>
        Already trading? <a className="link" href="/signin">Log in</a>
      </p>
    </form>
  );
}
