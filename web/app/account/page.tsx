import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { TotpSetup } from "./TotpSetup";
import { DeleteAccount, ExportData, NewCodes } from "./AccountControls";

export const dynamic = "force-dynamic";

export default async function Account() {
  const user = await me();
  if (!user) redirect("/signin");
  return (
    <div className="stack" style={{ maxWidth: 640 }}>
      <h1 className="display">{user.username}</h1>
      <p className="muted" style={{ margin: 0 }}>Role: {user.role}</p>
      <section className="card pad stack" aria-labelledby="totp-h">
        <div className="spread">
          <h2 id="totp-h" style={{ fontSize: 18 }}>Two-step sign-in</h2>
          <span className={`chip ${user.totp ? "ok" : ""}`}>{user.totp ? "On" : "Off"}</span>
        </div>
        {user.totp ? (
          <p style={{ margin: 0 }}>Signing in asks for a code from your authenticator app.</p>
        ) : (
          <>
            <p style={{ margin: 0 }} className="dim">Protect your account with a code from an authenticator app. Admin accounts must have it.</p>
            <TotpSetup />
          </>
        )}
        {user.role === "admin" && !user.signed_in_with_totp && (
          <p className="notice error">Admin actions need a sign-in with a code: sign out and sign in again.</p>
        )}
      </section>
      <section className="card pad stack" aria-labelledby="codes-h">
        <h2 id="codes-h" style={{ fontSize: 18 }}>Recovery codes</h2>
        <p style={{ margin: 0 }} className="dim">
          The only way back in if you forget your password. If you have lost them or used most of them, make new ones.
        </p>
        <NewCodes username={user.username} />
      </section>
      <section className="card pad stack" aria-labelledby="data-h">
        <h2 id="data-h" style={{ fontSize: 18 }}>Your data</h2>
        <p style={{ margin: 0 }} className="dim">
          A copy of everything Brier holds about you: your profile, orders, trades, forecasts and balance, as a JSON file.
        </p>
        <ExportData username={user.username} />
      </section>
      <section className="card pad stack danger-zone" aria-labelledby="delete-h">
        <h2 id="delete-h" style={{ fontSize: 18 }}>Delete account</h2>
        <DeleteAccount username={user.username} />
      </section>
    </div>
  );
}
