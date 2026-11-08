import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { TotpSetup } from "./TotpSetup";

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
    </div>
  );
}
