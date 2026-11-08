"use client";
import { useState } from "react";

/** Recovery codes, shown once: copy them or save them as a file before moving on. */
export function RecoveryCodes({ codes, username, onDone, doneLabel }: { codes: string[]; username: string; onDone: () => void; doneLabel: string }) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);
  const text = `Brier recovery codes for ${username}\nEach code resets your password once. Keep them somewhere safe.\n\n${codes.join("\n")}\n`;

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  function download() {
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `brier-recovery-codes-${username}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="stack" style={{ gap: 14 }}>
      <p className="dim" style={{ margin: 0 }}>
        Brier has no email, so these codes are the only way back in if you forget your password. Each works once.
        They are shown only now.
      </p>
      <ol className="codes" aria-label="Recovery codes">
        {codes.map((c) => (
          <li key={c}>
            <code>{c}</code>
          </li>
        ))}
      </ol>
      <div className="row" style={{ gap: 8 }}>
        <button type="button" className="btn" onClick={copy}>
          {copied ? "Copied" : "Copy all"}
        </button>
        <button type="button" className="btn" onClick={download}>
          Download .txt
        </button>
      </div>
      <label className="check">
        <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
        <span>I have saved my recovery codes</span>
      </label>
      <button type="button" className="btn primary block" disabled={!saved} onClick={onDone}>
        {doneLabel}
      </button>
    </div>
  );
}
