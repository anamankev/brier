"use client";
import Link from "next/link";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";
import { TERMS_VERSION } from "@/lib/terms";
import { RecoveryCodes } from "@/components/RecoveryCodes";

const REASONS: Record<string, string> = {
  username_taken: "That username is taken.",
  too_many_signups: "Too many accounts were opened from your network today. Please try again tomorrow.",
  signups_paused: "Sign-up is paused for today. Please try again tomorrow.",
  rate_limited: "Too many attempts. Wait a minute and try again.",
};

export function RegisterForm() {
  const router = useRouter();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [adult, setAdult] = useState(false);
  const [terms, setTerms] = useState(false);
  const [busy, setBusy] = useState(false);
  const [codes, setCodes] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const r = await api<{ recovery_codes: string[] }>("POST", "/v1/register", { username, password, adult, terms: TERMS_VERSION });
      await fetch("/api/session", {
        method: "POST",
        headers: { "content-type": "application/json", "x-brier-request": "1" },
        body: JSON.stringify({ username, password }),
      });
      setCodes(r.recovery_codes);
    } catch (err) {
      setError(err instanceof ApiError ? (REASONS[err.code] ?? err.message) : "Could not create the account.");
    } finally {
      setBusy(false);
    }
  }

  if (codes) {
    return (
      <div className="card auth">
        <h1 className="display">Save your recovery codes</h1>
        <RecoveryCodes
          codes={codes}
          username={username}
          doneLabel="Start trading"
          onDone={() => {
            router.push("/");
            router.refresh();
          }}
        />
      </div>
    );
  }

  return (
    <form className="card auth" onSubmit={submit}>
      <h1 className="display">Create your account</h1>
      <p className="muted" style={{ margin: 0 }}>
        Start with 10,000 play credits. They have no cash value and cannot be bought or withdrawn.
      </p>
      <label className="field">
        <span>Username (3–32 characters: a–z, 0–9, _)</span>
        <input autoComplete="username" pattern="[a-z0-9_]{3,32}" value={username} onChange={(e) => setUsername(e.target.value.toLowerCase())} required />
      </label>
      <label className="field">
        <span>Password (at least 12 characters)</span>
        <input type="password" autoComplete="new-password" minLength={12} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} required />
      </label>
      <label className="check">
        <input type="checkbox" checked={adult} onChange={(e) => setAdult(e.target.checked)} required />
        <span>I am 18 or older</span>
      </label>
      <label className="check">
        <input type="checkbox" checked={terms} onChange={(e) => setTerms(e.target.checked)} required />
        <span>
          I agree to the{" "}
          <Link className="link" href="/terms" target="_blank">
            terms of use
          </Link>{" "}
          and{" "}
          <Link className="link" href="/rules" target="_blank">
            house rules
          </Link>
          , and have read the{" "}
          <Link className="link" href="/privacy" target="_blank">
            privacy notice
          </Link>
        </span>
      </label>
      <button className="btn primary block" type="submit" disabled={busy || !adult || !terms}>
        {busy ? "Creating…" : "Create account"}
      </button>
      <div aria-live="assertive">{error && <p className="notice error">{error}</p>}</div>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>
        Already trading? <Link className="link" href="/signin">Log in</Link>
      </p>
    </form>
  );
}
