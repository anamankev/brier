"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { api, ApiError } from "@/lib/client";
import { QrCode } from "@/components/QrCode";

export function TotpSetup() {
  const router = useRouter();
  const [setup, setSetup] = useState<{ secret: string; otpauth_uri: string } | null>(null);
  const [code, setCode] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  if (!setup) {
    return (
      <button
        type="button"
        className="btn primary"
        onClick={async () => {
          try {
            setSetup(await api("POST", "/v1/totp/setup"));
          } catch (err) {
            setMessage({ ok: false, text: err instanceof ApiError ? err.message : "Could not start TOTP setup." });
          }
        }}
      >
        Turn on two-step sign-in
      </button>
    );
  }
  return (
    <form
      className="stack"
      onSubmit={async (e) => {
        e.preventDefault();
        try {
          await api("POST", "/v1/totp/enable", { code });
          setMessage({ ok: true, text: "Two-step sign-in is on." });
          router.refresh();
        } catch (err) {
          setMessage({ ok: false, text: err instanceof ApiError ? err.message : "That code did not work." });
        }
      }}
    >
      <p style={{ margin: 0 }}>
        Scan this with your authenticator app (1Password, Google Authenticator, Authy), then enter the 6-digit code it
        shows. The key stays the same if you reload this page.
      </p>
      <div className="totp-setup">
        <QrCode text={setup.otpauth_uri} label="QR code for your authenticator app" />
        <div className="stack" style={{ gap: 8 }}>
          <span className="muted" style={{ fontSize: 13 }}>Can&apos;t scan? Enter this key instead:</span>
          <code className="totp-key" data-testid="totp-secret">
            {setup.secret}
          </code>
          <div className="row" style={{ gap: 8 }}>
            <button type="button" className="btn small" onClick={() => navigator.clipboard?.writeText(setup.secret).catch(() => undefined)}>
              Copy key
            </button>
            <a className="btn small" href={setup.otpauth_uri}>
              Open in app
            </a>
          </div>
        </div>
      </div>
      <label className="field">
        <span>6-digit code</span>
        <input inputMode="numeric" autoComplete="one-time-code" pattern="[0-9]{6}" value={code} onChange={(e) => setCode(e.target.value)} required />
      </label>
      <button className="btn primary" type="submit">
        Turn on
      </button>
      <div aria-live="polite">{message && <p className={`notice ${message.ok ? "ok" : "error"}`}>{message.text}</p>}</div>
    </form>
  );
}
